use serde::Deserialize;
use wax_fmt::{is_supported, render, FmtValue};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Corpus {
    totals: Totals,
    formats: Vec<CorpusFormat>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Totals {
    formatted_cells: u64,
    distinct_codes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusFormat {
    code: String,
    cell_count: u64,
}

#[test]
fn supports_at_least_95_percent_of_mined_format_cells() {
    let corpus: Corpus =
        serde_json::from_str(include_str!("../../../harness/formats/corpus-formats.json"))
            .expect("committed corpus format set must be valid");
    assert_eq!(corpus.formats.len(), corpus.totals.distinct_codes);
    assert_eq!(
        corpus
            .formats
            .iter()
            .map(|format| format.cell_count)
            .sum::<u64>(),
        corpus.totals.formatted_cells
    );

    let supported_cells = corpus
        .formats
        .iter()
        .filter(|format| is_supported(&format.code))
        .map(|format| format.cell_count)
        .sum::<u64>();
    let coverage = supported_cells as f64 / corpus.totals.formatted_cells as f64;
    let unsupported = corpus
        .formats
        .iter()
        .filter(|format| !is_supported(&format.code))
        .collect::<Vec<_>>();

    println!(
        "wax-fmt corpus coverage: {supported_cells}/{} cells ({:.4}%), {}/{} distinct codes",
        corpus.totals.formatted_cells,
        coverage * 100.0,
        corpus.formats.len() - unsupported.len(),
        corpus.formats.len(),
    );
    if !unsupported.is_empty() {
        println!("highest-frequency unsupported codes:");
        for format in unsupported.iter().take(20) {
            println!("  {:>8}  {}", format.cell_count, format.code);
        }
    }

    assert!(
        coverage >= 0.95,
        "supported cell-frequency share {:.4}% is below the 95% W2 gate",
        coverage * 100.0
    );
}

#[test]
fn rendering_never_panics_on_corpus_codes_or_generated_junk() {
    let corpus: Corpus =
        serde_json::from_str(include_str!("../../../harness/formats/corpus-formats.json"))
            .expect("committed corpus format set must be valid");
    let mut codes = corpus
        .formats
        .into_iter()
        .map(|format| format.code)
        .collect::<Vec<_>>();

    let alphabet = [
        '0', '#', '?', '.', ',', ';', '[', ']', '"', '\\', '_', '*', '@', '%', 'y', 'm', 'd', 'h',
        's', 'E', '+', '-', '/', ' ', 'x', '💥',
    ];
    let mut state = 0x9e37_79b9_u64;
    for _ in 0..4096 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let length = (state as usize % 40) + 1;
        let mut code = String::new();
        for _ in 0..length {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            code.push(alphabet[state as usize % alphabet.len()]);
        }
        codes.push(code);
    }

    for code in &codes {
        let _ = is_supported(code);
        for epoch_1904 in [false, true] {
            for number in [
                f64::MIN,
                -12_410.5,
                -0.0,
                0.0,
                0.1 + 0.2,
                60.0,
                45_205.543_219_9,
                f64::MAX,
                f64::NAN,
                f64::INFINITY,
            ] {
                let _ = render(code, FmtValue::Number(number), epoch_1904);
            }
            let _ = render(code, FmtValue::Text("text"), epoch_1904);
            let _ = render(code, FmtValue::Bool(true), epoch_1904);
            let _ = render(code, FmtValue::Error("#N/A"), epoch_1904);
        }
    }
}
