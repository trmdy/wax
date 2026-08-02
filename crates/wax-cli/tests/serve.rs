use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rust_xlsxwriter::{Formula, Workbook};
use serde_json::{json, Value};

struct Server {
    child: Child,
    input: Option<ChildStdin>,
    responses: mpsc::Receiver<String>,
}

impl Server {
    fn start(arguments: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_wax"))
            .arg("serve")
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("wax serve should spawn");
        let input = child.stdin.take().expect("server stdin should be piped");
        let output = child.stdout.take().expect("server stdout should be piped");
        let (sender, responses) = mpsc::channel();
        thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut line = String::new();
                match output.read_line(&mut line) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        if sender.send(line).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Self {
            child,
            input: Some(input),
            responses,
        }
    }

    fn send(&mut self, request: Value) {
        let input = self.input.as_mut().expect("server stdin should be open");
        serde_json::to_writer(&mut *input, &request).expect("request should serialize");
        input.write_all(b"\n").expect("request should write");
        input.flush().expect("request should flush");
    }

    fn send_raw(&mut self, line: &str) {
        let input = self.input.as_mut().expect("server stdin should be open");
        input.write_all(line.as_bytes()).expect("line should write");
        input.write_all(b"\n").expect("newline should write");
        input.flush().expect("line should flush");
    }

    fn receive(&self) -> Value {
        let line = self
            .responses
            .recv_timeout(Duration::from_secs(5))
            .expect("server should answer within five seconds");
        serde_json::from_str(&line).expect("server response should be valid JSON")
    }

    fn eof(mut self) -> ExitStatus {
        self.input.take();
        self.child.wait().expect("server should exit after EOF")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.input.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("wax-read")
        .join("tests")
        .join("fixtures")
        .join("reader.xlsx")
}

fn open_path(server: &mut Server, id: u64, path: &Path) -> Value {
    server.send(json!({
        "id": id,
        "op": "open",
        "path": path,
        "timeoutMs": 10_000
    }));
    let response = server.receive();
    assert_eq!(response["id"], id);
    assert_eq!(response["ok"], true, "{response}");
    response
}

fn open(server: &mut Server, id: u64) -> Value {
    open_path(server, id, &fixture_path())
}

fn write_second_formula_fixture(path: &Path) {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .write_number(0, 0, 4.0)
        .expect("precedent should write");
    worksheet
        .write_number(0, 1, 3.0)
        .expect("second precedent should write");
    worksheet
        .write_formula(0, 2, Formula::new("A1*2+B1").set_result("11"))
        .expect("formula should write");
    workbook.save(path).expect("formula fixture should save");
}

#[test]
fn open_and_meta_carry_declared_sizes_and_resolved_defaults() {
    let fixture = fixture_path().with_file_name("sizes.xlsx");
    let mut server = Server::start(&[]);
    let opened = open_path(&mut server, 40, &fixture);
    let expected = json!([{
        "name":"Sizes","rows":3,"cols":2,"truncated":false,
        "colInfos":[{"c":1,"width":22.5}],
        "rowInfos":[
            {"r":0,"height":27.75},
            {"r":2,"height":30.6},
            {"r":5,"height":45.0}
        ],
        "defaultRowHeight":14.4,
        "defaultColWidth":9.14,
        "frozenRows":0,
        "frozenCols":0
    }]);
    assert_eq!(opened["sheets"], expected);

    server.send(json!({"id":41,"op":"meta","handle":opened["handle"]}));
    let meta = server.receive();
    assert_eq!(meta["ok"], true, "{meta}");
    assert_eq!(meta["sheets"], expected);
    assert!(server.eof().success());
}

#[test]
fn export_applies_size_overrides_and_reopen_shows_them() {
    let fixture = fixture_path().with_file_name("sizes.xlsx");
    let temp = tempfile::tempdir().expect("temporary directory");
    let out = temp.path().join("resized.xlsx");
    let mut server = Server::start(&[]);
    let opened = open_path(&mut server, 50, &fixture);

    server.send(json!({
        "id":51,"op":"export","handle":opened["handle"],"format":"xlsx","out":out,
        "sizeOverrides":{
            "cols":[{"sheet":0,"c":1,"width":33.75},{"sheet":0,"c":4,"width":12.5}],
            "rows":[{"sheet":0,"r":0,"height":50.1}]
        }
    }));
    let exported = server.receive();
    assert_eq!(exported["ok"], true, "{exported}");

    let reopened = open_path(&mut server, 52, &out);
    let sheet = &reopened["sheets"][0];
    assert_eq!(
        sheet["colInfos"],
        json!([{"c":1,"width":33.75},{"c":4,"width":12.5}])
    );
    assert_eq!(
        sheet["rowInfos"],
        json!([
            {"r":0,"height":50.1},
            {"r":2,"height":30.6},
            {"r":5,"height":45.0}
        ])
    );
    assert_eq!(sheet["defaultRowHeight"], json!(14.4));
    assert_eq!(sheet["defaultColWidth"], json!(9.14));

    // Malformed size overrides answer bad_request naming the field.
    server.send(json!({
        "id":53,"op":"export","handle":opened["handle"],"format":"xlsx",
        "out":temp.path().join("bad.xlsx"),
        "sizeOverrides":{"rows":[{"sheet":0,"r":0}]}
    }));
    let error = server.receive();
    assert_eq!(error["ok"], false);
    assert_eq!(error["code"], "bad_request");
    assert_eq!(
        error["msg"],
        "sizeOverrides.rows[0].height must be a finite number"
    );
    assert!(server.eof().success());
}

#[test]
fn version_handshake_and_eof_exit_zero() {
    let mut server = Server::start(&[]);
    server.send(json!({"id": 1, "op": "version"}));
    let response = server.receive();
    assert_eq!(
        response,
        json!({
            "id": 1,
            "ok": true,
            "proto": 0,
            "version": env!("CARGO_PKG_VERSION"),
            "caps": ["exportOverrides", "sheetSizeInfos", "exportSizeOverrides", "formulaEval", "sheetView"],
        })
    );
    assert!(server.eof().success());
}

#[test]
fn open_meta_window_close_happy_path() {
    let mut server = Server::start(&[]);
    let opened = open(&mut server, 10);
    assert_eq!(opened["proto"], 0);
    assert_eq!(
        opened["caps"],
        json!([
            "exportOverrides",
            "sheetSizeInfos",
            "exportSizeOverrides",
            "formulaEval",
            "sheetView"
        ])
    );
    assert_eq!(opened["handle"], "h1");
    assert_eq!(opened["truncated"], false);
    // The sheetSizeInfos contract: all four size fields always present,
    // defaults resolved to the Excel fallbacks when the file declares none.
    assert_eq!(
        opened["sheets"],
        json!([{
            "name":"Reader","rows":4,"cols":7,"truncated":false,
            "colInfos":[
                {"c":0,"width":12.5},{"c":1,"width":20.0},{"c":2,"width":20.0},
                {"c":3,"width":20.0},{"c":6,"width":15.25}
            ],
            "rowInfos":[],
            "defaultRowHeight":15.0,
            "defaultColWidth":8.43,
            "frozenRows":0,
            "frozenCols":0
        }])
    );

    server.send(json!({"id":11,"op":"meta","handle":"h1"}));
    let meta = server.receive();
    assert_eq!(meta["id"], 11);
    assert_eq!(meta["sheets"], opened["sheets"]);
    assert!(meta.get("proto").is_none());
    assert!(meta.get("handle").is_none());

    server.send(json!({
        "id":12,"op":"window","handle":"h1","sheet":0,
        "r0":0,"c0":0,"nr":2,"nc":6
    }));
    let window = server.receive();
    assert_eq!(window["id"], 12);
    assert_eq!(window["ok"], true);
    assert_eq!(
        (&window["r0"], &window["c0"], &window["nr"], &window["nc"]),
        (&json!(0), &json!(0), &json!(2), &json!(6))
    );
    assert_eq!(window["rows"].as_array().expect("rows").len(), 2);
    assert_eq!(window["rows"][0].as_array().expect("cells").len(), 6);
    assert_eq!(
        window["rows"][0][0],
        json!({"t":"s","v":"Hello shared","d":"Hello shared"})
    );
    assert_eq!(window["rows"][1][2]["f"], "SUM(A2:B2)");
    assert_eq!(window["rows"][1][2]["v"], 5.0);
    assert!(window["rows"][1][2].get("d").is_none());
    assert_eq!(window["rows"][1][2]["e"], true);
    assert_eq!(window["rows"][1][3]["e"], true);
    assert_eq!(window["rows"][1][4]["v"], "#DIV/0!");
    assert_eq!(window["rows"][1][4]["e"], true);
    assert!(window["rows"][1][5].get("e").is_none());
    assert_eq!(window["merges"], json!([]));

    server.send(json!({"id":13,"op":"stats"}));
    let stats = server.receive();
    assert_eq!(stats["handles"], 1);
    assert!(stats["peakRssBytes"].as_u64().expect("RSS is a u64") > 0);
    assert!(stats["storeBytes"].as_u64().expect("store bytes") > 0);

    server.send(json!({"id":14,"op":"close","handle":"h1"}));
    assert_eq!(server.receive(), json!({"id":14,"ok":true}));
    server.send(json!({"id":15,"op":"meta","handle":"h1"}));
    let closed = server.receive();
    assert_eq!(closed["code"], "bad_handle");
    assert!(!closed["msg"].as_str().expect("message").contains("expired"));
    assert!(server.eof().success());
}

#[test]
fn recalc_is_incremental_side_effect_free_and_export_round_trips_it() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let out = temp.path().join("recalculated.xlsx");
    let mut server = Server::start(&[]);
    let opened = open(&mut server, 60);

    server.send(json!({
        "id":61,"op":"recalc","handle":opened["handle"],
        "overrides":[
            {"sheet":0,"r":1,"c":0,"v":4},
            {"sheet":0,"r":1,"c":0,"v":10}
        ]
    }));
    let recalculated = server.receive();
    assert_eq!(
        recalculated,
        json!({
            "id":61,"ok":true,
            "changed":[{"sheet":0,"r":1,"c":2,"v":13.0,"d":null,"e":true}],
            "evaluated":2,"skipped":0,"truncated":false,"warnings":[]
        })
    );

    // Recalc is a hypothetical layer: it does not mutate the handle's
    // subsequent windows.
    server.send(json!({
        "id":62,"op":"window","handle":opened["handle"],"sheet":0,
        "r0":1,"c0":0,"nr":1,"nc":3
    }));
    let base = server.receive();
    assert_eq!(base["rows"][0][0]["v"], 2.0);
    assert_eq!(base["rows"][0][2]["v"], 5.0);
    assert_eq!(base["rows"][0][2]["e"], true);

    server.send(json!({
        "id":63,"op":"export","handle":opened["handle"],"format":"xlsx","out":out,
        "overrides":[{"sheet":0,"r":1,"c":0,"v":10}]
    }));
    let exported = server.receive();
    assert_eq!(exported["ok"], true, "{exported}");
    assert_eq!(exported["applied"], 1);

    let reopened = open_path(&mut server, 64, &out);
    server.send(json!({
        "id":65,"op":"window","handle":reopened["handle"],"sheet":0,
        "r0":1,"c0":0,"nr":1,"nc":3
    }));
    let round_trip = server.receive();
    assert_eq!(round_trip["rows"][0][0]["v"], 10.0);
    assert_eq!(round_trip["rows"][0][2]["f"], "SUM(A2:B2)");
    assert_eq!(round_trip["rows"][0][2]["v"], 13.0);
    assert_eq!(round_trip["rows"][0][2]["e"], true);
    assert!(server.eof().success());
}

#[test]
fn recalc_matches_full_reopen_across_formula_corpus_samples() {
    struct Sample {
        path: PathBuf,
        precedent: (u32, u32),
        formula: (u32, u32),
    }
    let temp = tempfile::tempdir().expect("temporary directory");
    let second_fixture = temp.path().join("second-formula-fixture.xlsx");
    write_second_formula_fixture(&second_fixture);
    let samples = [
        Sample {
            path: fixture_path(),
            precedent: (1, 0),
            formula: (1, 2),
        },
        Sample {
            path: second_fixture,
            precedent: (0, 0),
            formula: (0, 2),
        },
    ];
    let mut server = Server::start(&[]);
    let mut id = 100_u64;

    for (sample_index, sample) in samples.iter().enumerate() {
        let opened = open_path(&mut server, id, &sample.path);
        id += 1;
        let source_handle = opened["handle"].as_str().expect("source handle").to_owned();
        for (value_index, value) in [-3.0, 0.0, 10.25].into_iter().enumerate() {
            server.send(json!({
                "id":id,"op":"recalc","handle":source_handle,
                "overrides":[{
                    "sheet":0,"r":sample.precedent.0,"c":sample.precedent.1,"v":value
                }]
            }));
            let recalculated = server.receive();
            assert_eq!(recalculated["ok"], true, "{recalculated}");
            let expected = recalculated["changed"]
                .as_array()
                .expect("changed array")
                .iter()
                .find(|cell| {
                    cell["sheet"] == 0
                        && cell["r"] == sample.formula.0
                        && cell["c"] == sample.formula.1
                })
                .unwrap_or_else(|| panic!("formula missing from changed set: {recalculated}"))["v"]
                .clone();
            id += 1;

            let out = temp
                .path()
                .join(format!("sample-{sample_index}-{value_index}.xlsx"));
            server.send(json!({
                "id":id,"op":"export","handle":source_handle,"format":"xlsx","out":out,
                "overrides":[{
                    "sheet":0,"r":sample.precedent.0,"c":sample.precedent.1,"v":value
                }]
            }));
            let exported = server.receive();
            assert_eq!(exported["ok"], true, "{exported}");
            id += 1;

            let reopened = open_path(&mut server, id, &out);
            id += 1;
            server.send(json!({
                "id":id,"op":"window","handle":reopened["handle"],"sheet":0,
                "r0":sample.formula.0,"c0":sample.formula.1,"nr":1,"nc":1
            }));
            let full = server.receive();
            assert_eq!(full["rows"][0][0]["e"], true, "{full}");
            assert_eq!(
                full["rows"][0][0]["v"], expected,
                "sample {sample_index}, override {value}"
            );
            id += 1;
        }
    }
    assert!(server.eof().success());
}

#[test]
fn windows_clip_report_merges_and_reject_invalid_requests() {
    let mut server = Server::start(&[]);
    open(&mut server, 20);

    server.send(json!({
        "id":21,"op":"window","handle":"h1","sheet":0,
        "r0":3,"c0":6,"nr":10,"nc":10
    }));
    let clipped = server.receive();
    assert_eq!(
        (
            &clipped["r0"],
            &clipped["c0"],
            &clipped["nr"],
            &clipped["nc"]
        ),
        (&json!(3), &json!(6), &json!(1), &json!(1))
    );
    assert_eq!(clipped["rows"], json!([[null]]));

    server.send(json!({
        "id":22,"op":"window","handle":"h1","sheet":0,
        "r0":4,"c0":0,"nr":1,"nc":1
    }));
    let outside = server.receive();
    assert_eq!(outside["ok"], true);
    assert_eq!(outside["r0"], 4);
    assert_eq!(outside["c0"], 0);
    assert_eq!(outside["nr"], 0);
    assert_eq!(outside["nc"], 0);
    assert_eq!(outside["rows"], json!([]));

    server.send(json!({
        "id":23,"op":"window","handle":"h1","sheet":0,
        "r0":2,"c0":0,"nr":1,"nc":2
    }));
    assert_eq!(server.receive()["merges"], json!(["A3:B3"]));

    server.send(json!({
        "id":24,"op":"window","handle":"h1","sheet":0,
        "r0":0,"c0":0,"nr":513,"nc":512
    }));
    assert_eq!(server.receive()["code"], "bad_request");
    server.send(json!({
        "id":25,"op":"window","handle":"h1","sheet":99,
        "r0":0,"c0":0,"nr":1,"nc":1
    }));
    assert_eq!(server.receive()["code"], "bad_request");
    assert!(server.eof().success());
}

#[test]
fn csv_export_is_rfc_4180_and_xlsx_succeeds() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let out = temp.path().join("reader.csv");
    let mut server = Server::start(&[]);
    open(&mut server, 30);

    server.send(json!({
        "id":31,"op":"export","handle":"h1","format":"csv","out":out
    }));
    let response = server.receive();
    let expected = concat!(
        "Hello shared,TRUE,#DIV/0!,1904-01-01,01-02-04,Rich text,\"1,234.50\"\r\n",
        "2,3,5,xy,,,12.3456 widgets\r\n",
        "Merged,,,,,,\r\n",
        ",,,,,,\r\n",
    )
    .as_bytes();
    assert_eq!(std::fs::read(&out).expect("CSV should exist"), expected);
    assert_eq!(response["bytes"], expected.len());
    assert_eq!(
        response["dropped"],
        json!([
            "formulas (cached values only)",
            "number formatting beyond display strings",
            "merges",
            "styles",
            "column widths",
            "row heights",
            "formulas kept file-cached values (1 unevaluated)"
        ])
    );

    // With the W4A writer merged, xlsx export over serve succeeds for real.
    let copy = temp.path().join("copy.xlsx");
    server.send(json!({
        "id":32,"op":"export","handle":"h1","format":"xlsx","out":copy
    }));
    let xlsx = server.receive();
    assert_eq!(xlsx["ok"], true, "{xlsx}");
    assert!(xlsx["bytes"].as_u64().expect("bytes") > 0);
    assert!(xlsx["dropped"].is_array());
    assert!(copy.exists());
    assert!(server.eof().success());
}

#[test]
fn export_applies_overrides_and_reports_the_post_collapse_count() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let out = temp.path().join("edited.csv");
    let mut server = Server::start(&[]);
    open(&mut server, 40);

    // Duplicate overrides collapse last-wins; a clear empties a cell; the
    // response carries the post-collapse applied count.
    server.send(json!({
        "id": 41, "op": "export", "handle": "h1", "format": "csv", "out": out,
        "overrides": [
            {"sheet": 0, "r": 0, "c": 0, "v": "stale"},
            {"sheet": 0, "r": 0, "c": 0, "v": "edited"},
            {"sheet": 0, "r": 0, "c": 1, "v": null},
        ]
    }));
    let response = server.receive();
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["applied"], 2, "{response}");
    let csv = std::fs::read_to_string(&out).expect("CSV should exist");
    let first_line = csv.lines().next().expect("csv should have a first line");
    assert!(
        first_line.starts_with("edited,,"),
        "override + clear must land in the first row: {first_line}"
    );

    // The same edit set through xlsx: applied covers the whole workbook.
    let copy = temp.path().join("edited.xlsx");
    server.send(json!({
        "id": 42, "op": "export", "handle": "h1", "format": "xlsx", "out": copy,
        "overrides": [{"sheet": 0, "r": 0, "c": 0, "v": 42.5}]
    }));
    let response = server.receive();
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["applied"], 1, "{response}");
    assert!(copy.exists());

    // A5: unknown sheet index is bad_request, never a silent skip; the
    // malformed-entry taxonomy also comes back bad_request naming the field.
    let refused = temp.path().join("refused.csv");
    server.send(json!({
        "id": 43, "op": "export", "handle": "h1", "format": "csv", "out": refused,
        "overrides": [{"sheet": 9, "r": 0, "c": 0, "v": 1}]
    }));
    let response = server.receive();
    assert_eq!(response["code"], "bad_request", "{response}");
    assert!(!refused.exists());
    server.send(json!({
        "id": 44, "op": "export", "handle": "h1", "format": "csv", "out": refused,
        "overrides": [{"sheet": 0, "r": 0, "c": 0}]
    }));
    let response = server.receive();
    assert_eq!(response["code"], "bad_request", "{response}");
    assert!(response["msg"]
        .as_str()
        .expect("message")
        .contains("overrides[0].v"));
    assert!(!refused.exists());
    assert!(server.eof().success());
}

#[test]
fn export_validates_handle_sheet_format_and_unwritable_output() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let mut server = Server::start(&[]);

    server.send(json!({
        "id": 33,
        "op": "export",
        "handle": "missing",
        "format": "csv",
        "out": temp.path().join("bad-handle.csv")
    }));
    assert_eq!(server.receive()["code"], "bad_handle");
    open(&mut server, 34);

    for (id, format) in [(35, "csv"), (36, "xlsx")] {
        server.send(json!({
            "id": id,
            "op": "export",
            "handle": "h1",
            "format": format,
            "out": temp.path().join(format!("bad-sheet.{format}")),
            "sheet": 99
        }));
        assert_eq!(server.receive()["code"], "bad_request");
    }

    server.send(json!({
        "id": 37,
        "op": "export",
        "handle": "h1",
        "format": "pdf",
        "out": temp.path().join("copy.pdf")
    }));
    assert_eq!(server.receive()["code"], "unsupported");

    let unwritable = temp.path().join("missing-parent").join("copy.csv");
    server.send(json!({
        "id": 38,
        "op": "export",
        "handle": "h1",
        "format": "csv",
        "out": unwritable
    }));
    let failure = server.receive();
    assert_eq!(failure["code"], "internal");
    assert!(failure["msg"]
        .as_str()
        .expect("message")
        .contains(&unwritable.display().to_string()));
    assert!(!unwritable.exists());
    assert!(server.eof().success());
}

#[test]
fn export_appends_open_warnings_to_dropped() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("wax-read")
        .join("tests")
        .join("fixtures")
        .join("date_1904.xlsb");
    let temp = tempfile::tempdir().expect("temporary directory");
    let out = temp.path().join("date.csv");
    let mut server = Server::start(&[]);
    let opened = open_path(&mut server, 39, &fixture);
    assert_eq!(
        opened["warnings"],
        json!(["xlsb merged regions are best-effort"])
    );

    server.send(json!({
        "id":40,"op":"export","handle":"h1","format":"csv","out":out
    }));
    let exported = server.receive();
    assert_eq!(exported["ok"], true);
    assert_eq!(
        exported["dropped"],
        json!([
            "formulas (cached values only)",
            "number formatting beyond display strings",
            "merges",
            "styles",
            "column widths",
            "row heights",
            "xlsb merged regions are best-effort"
        ])
    );
    assert!(server.eof().success());
}

#[test]
fn xlsx_export_round_trips_values_and_merges() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let out = temp.path().join("copy.xlsx");
    let mut server = Server::start(&[]);
    open(&mut server, 80);
    server.send(json!({
        "id":81,"op":"export","handle":"h1","format":"xlsx","out":out
    }));
    let exported = server.receive();
    assert_eq!(exported["ok"], true);
    assert!(exported["bytes"].as_u64().expect("byte count") > 0);
    assert!(server.eof().success());

    let dumped = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["dump", "--json"])
        .arg(&out)
        .output()
        .expect("wax dump should execute");
    assert!(dumped.status.success());
    let document: Value = serde_json::from_slice(&dumped.stdout).expect("dump should contain JSON");
    assert_eq!(document["ok"], true);
    assert_eq!(document["sheets"][0]["cells"][0]["v"], "Hello shared");
    assert_eq!(document["sheets"][0]["merges"][0], "A3:B3");
}
#[test]
fn caps_max_handles_and_idle_expiry_are_enforced() {
    let mut server = Server::start(&["--max-handles", "1", "--idle-timeout-ms", "30"]);
    open(&mut server, 40);

    server.send(json!({
        "id":41,"op":"open","path":fixture_path(),"timeoutMs":10_000
    }));
    let capped = server.receive();
    assert_eq!(capped["code"], "bad_request");
    assert!(capped["msg"].as_str().expect("message").contains("maximum"));

    thread::sleep(Duration::from_millis(80));
    server.send(json!({"id":42,"op":"meta","handle":"h1"}));
    let expired = server.receive();
    assert_eq!(expired["code"], "bad_handle");
    assert!(expired["msg"]
        .as_str()
        .expect("message")
        .contains("expired"));

    server.send(json!({
        "id":43,"op":"open","path":fixture_path(),"maxBytes":1
    }));
    assert_eq!(server.receive()["code"], "too_large");

    server.send(json!({
        "id":44,"op":"open","path":fixture_path(),"maxCells":2
    }));
    let truncated = server.receive();
    assert_eq!(truncated["handle"], "h2");
    assert_eq!(truncated["truncated"], true);
    assert_eq!(truncated["sheets"][0]["truncated"], true);
    assert!(server.eof().success());
}

#[test]
fn malformed_lines_never_panic_and_receive_bad_request() {
    let mut server = Server::start(&[]);
    let cases = [
        ("", None),
        ("not json", None),
        ("[]", None),
        ("{}", None),
        (r#"{"id":-1,"op":"version"}"#, None),
        (r#"{"id":"1","op":"version"}"#, None),
        (r#"{"id":1,"op":"wat"}"#, Some(1)),
        (
            r#"{"id":2,"op":"window","handle":"h1","sheet":0,"r0":0,"c0":0,"nr":999999,"nc":999999}"#,
            Some(2),
        ),
        (
            r#"{"id":3,"op":"open","path":"x.xlsx","maxCells":null}"#,
            Some(3),
        ),
        (r#"{"id":18446744073709551616,"op":"stats"}"#, None),
    ];
    for (line, expected_id) in cases {
        server.send_raw(line);
        let response = server.receive();
        assert_eq!(response["id"].as_u64(), expected_id, "{response}");
        assert_eq!(response["ok"], false);
        assert_eq!(response["code"], "bad_request");
    }
    server.send(json!({"id":99,"op":"version"}));
    assert_eq!(server.receive()["ok"], true);
    assert!(server.eof().success());
}

#[cfg(unix)]
fn fifo_path(directory: &Path, name: &str) -> PathBuf {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = directory.join(name);
    let encoded = CString::new(path.as_os_str().as_bytes()).expect("path should not contain NUL");
    // SAFETY: encoded is a valid, NUL-terminated path and the mode is valid.
    let result = unsafe { libc::mkfifo(encoded.as_ptr(), 0o600) };
    assert_eq!(result, 0, "mkfifo should succeed");
    path
}

#[cfg(unix)]
#[test]
fn cancel_blocked_open_and_preserve_out_of_order_ids() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let fifo = fifo_path(temp.path(), "blocked.xlsx");
    let mut server = Server::start(&[]);
    server.send(json!({
        "id":50,"op":"open","path":fifo,"timeoutMs":10_000
    }));
    server.send(json!({"id":51,"op":"version"}));
    let version = server.receive();
    assert_eq!(version["id"], 51);
    assert_eq!(version["ok"], true);

    server.send(json!({"id":52,"op":"cancel","target":50}));
    assert_eq!(server.receive(), json!({"id":52,"ok":true,"found":true}));
    let cancelled = server.receive();
    assert_eq!(cancelled["id"], 50);
    assert_eq!(cancelled["code"], "cancelled");
    assert!(server.eof().success());
}

#[cfg(unix)]
#[test]
fn blocked_open_hits_wall_clock_timeout_and_server_survives() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let fifo = fifo_path(temp.path(), "timeout.xlsx");
    let mut server = Server::start(&[]);
    server.send(json!({
        "id":60,"op":"open","path":fifo,"timeoutMs":25
    }));
    let timeout = server.receive();
    assert_eq!(timeout["id"], 60);
    assert_eq!(timeout["code"], "timeout");
    server.send(json!({"id":61,"op":"version"}));
    assert_eq!(server.receive()["id"], 61);
    assert!(server.eof().success());
}

#[cfg(unix)]
#[test]
fn sigterm_exits_cleanly() {
    let mut server = Server::start(&[]);
    server.send(json!({"id":70,"op":"version"}));
    assert_eq!(server.receive()["ok"], true);
    // SAFETY: the child pid belongs to the live server process.
    assert_eq!(
        unsafe { libc::kill(server.child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    server.input.take();
    assert!(server.child.wait().expect("server should exit").success());
}
