//! A deliberately small, dependency-free Excel formula evaluator.
//!
//! The v0.4 engine evaluates only the curated function/operator surface in
//! `docs/v0.4-formula-eval-contract.md`. Unsupported syntax and functions are
//! retained as file-cached formulas rather than guessed. The immutable
//! [`WorkbookStore`] remains the source model; evaluated values are overlays,
//! which keeps `recalc` and export side-effect-free.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use calamine::{ExcelDateTime, ExcelDateTimeType};
use chrono::{NaiveDate, NaiveDateTime};
use wax_core::{CellOverride, CellType, CellValue, EXPORT_OVERRIDES_CAP};
use wax_fmt::FmtValue;
use wax_store::{WindowCell, WorkbookStore};

pub const DEFAULT_EVAL_BUDGET: Duration = Duration::from_millis(1_000);
pub const RECALC_CHANGED_CAP: usize = 50_000;
const OVERRIDE_EXTENT_CAP_CELLS: u64 = 8_000_000;
const MAX_FORMULA_BYTES: usize = 1_048_576;
const MAX_AST_NODES: usize = 4_096;
const MAX_PARSE_DEPTH: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CellRef {
    pub sheet: u32,
    pub r: u32,
    pub c: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatedCell {
    pub sheet: u32,
    pub r: u32,
    pub c: u32,
    pub t: CellType,
    pub v: Option<CellValue>,
    pub d: Option<String>,
    pub e: bool,
}

impl EvaluatedCell {
    pub fn apply(&self, cell: &mut WindowCell) {
        cell.t = self.t;
        cell.v.clone_from(&self.v);
        cell.d.clone_from(&self.d);
        cell.e = self.e;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EvaluationStats {
    pub evaluated: u64,
    pub skipped: u64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecalcOutcome {
    pub changed: Vec<EvaluatedCell>,
    /// Evaluated dirty-formula overlay after layering the recalc overrides.
    /// Export consumes this; it is not serialized on the wire.
    pub all_evaluated: Vec<EvaluatedCell>,
    pub evaluated: u64,
    pub skipped: u64,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvalError {
    pub code: &'static str,
    pub msg: String,
}

impl EvalError {
    fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            code: "bad_request",
            msg: msg.into(),
        }
    }

    fn bomb(msg: impl Into<String>) -> Self {
        Self {
            code: "bomb",
            msg: msg.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FormulaWorkbook {
    nodes: Vec<FormulaNode>,
    node_by_cell: HashMap<CellRef, usize>,
    topo: Vec<usize>,
    cycle_or_downstream: HashSet<usize>,
    base_results: HashMap<CellRef, EvaluatedCell>,
    formula_cells: u64,
    formula_cells_by_sheet: Vec<u64>,
}

#[derive(Clone, Debug)]
struct FormulaNode {
    cell: CellRef,
    ast: Expr,
    deps: Vec<Dependency>,
    fmt: Option<String>,
    cached_type: CellType,
    authored: bool,
}

type FormulaColumns = Vec<(u32, usize)>;
type FormulaRowIndex = HashMap<u32, BTreeMap<u32, FormulaColumns>>;

impl FormulaWorkbook {
    /// Build a file-cache-only workbook for containers outside the v0.4
    /// XLSX/XLSM evaluation scope. Recalc still validates its override
    /// layer, but no formula is marked evaluated.
    pub fn file_cached(store: &WorkbookStore) -> (Self, EvaluationStats) {
        let mut formula_cells = 0_u64;
        let mut formula_cells_by_sheet = Vec::new();
        for sheet in 0..store.sheet_count() {
            let mut sheet_formula_cells = 0_u64;
            let _ = store.scan_sheet(sheet, |_, _, cell| {
                if cell.f.is_some() {
                    formula_cells = formula_cells.saturating_add(1);
                    sheet_formula_cells = sheet_formula_cells.saturating_add(1);
                }
            });
            formula_cells_by_sheet.push(sheet_formula_cells);
        }
        (
            Self {
                nodes: Vec::new(),
                node_by_cell: HashMap::new(),
                topo: Vec::new(),
                cycle_or_downstream: HashSet::new(),
                base_results: HashMap::new(),
                formula_cells,
                formula_cells_by_sheet,
            },
            EvaluationStats {
                evaluated: 0,
                skipped: formula_cells,
                warnings: Vec::new(),
            },
        )
    }

    /// Build the dependency graph once and evaluate the curated formula set.
    /// Unsupported formulas remain represented by their file-cached store
    /// values and are included in `skipped`.
    pub fn open(store: &WorkbookStore, budget: Duration) -> (Self, EvaluationStats) {
        let sheet_names = (0..store.sheet_count())
            .filter_map(|sheet| {
                store
                    .sheet_meta(sheet)
                    .map(|meta| (meta.name.to_ascii_lowercase(), sheet))
            })
            .collect::<HashMap<_, _>>();
        let mut nodes = Vec::new();
        let mut formula_cells = 0_u64;
        let mut formula_cells_by_sheet = Vec::new();
        for sheet in 0..store.sheet_count() {
            let mut sheet_formula_cells = 0_u64;
            let _ = store.scan_sheet(sheet, |r, c, cell| {
                let Some(formula) = cell.f.as_deref() else {
                    return;
                };
                formula_cells = formula_cells.saturating_add(1);
                sheet_formula_cells = sheet_formula_cells.saturating_add(1);
                let current = CellRef { sheet, r, c };
                if let Ok(ast) = parse_formula(formula, sheet, &sheet_names) {
                    let mut deps = Vec::new();
                    ast.dependencies(&mut deps);
                    nodes.push(FormulaNode {
                        cell: current,
                        ast,
                        deps,
                        fmt: cell.fmt,
                        cached_type: cell.t,
                        authored: false,
                    });
                }
            });
            formula_cells_by_sheet.push(sheet_formula_cells);
        }

        let node_by_cell = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.cell, index))
            .collect::<HashMap<_, _>>();
        let formula_rows = formula_row_index(&nodes);
        let mut dependencies = vec![Vec::new(); nodes.len()];
        let mut reverse = vec![Vec::new(); nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            let mut formula_dependencies = HashSet::new();
            for dependency in &node.deps {
                match dependency {
                    Dependency::Cell(cell) => {
                        if let Some(&other) = node_by_cell.get(cell) {
                            formula_dependencies.insert(other);
                        }
                    }
                    Dependency::Range(range) => {
                        collect_formula_nodes_in_range(
                            &formula_rows,
                            *range,
                            &mut formula_dependencies,
                        );
                    }
                }
            }
            dependencies[index].extend(formula_dependencies);
            dependencies[index].sort_unstable();
            for &dependency in &dependencies[index] {
                reverse[dependency].push(index);
            }
        }
        for dependents in &mut reverse {
            dependents.sort_unstable();
            dependents.dedup();
        }
        let (topo, cycle_or_downstream) = topological_order(&dependencies, &reverse);
        let mut workbook = Self {
            nodes,
            node_by_cell,
            topo,
            cycle_or_downstream,
            base_results: HashMap::new(),
            formula_cells,
            formula_cells_by_sheet,
        };
        let (base_results, stats) = workbook.evaluate_all(store, budget);
        workbook.base_results = base_results;
        (workbook, stats)
    }

    pub fn base_evaluated(&self) -> Vec<EvaluatedCell> {
        let mut cells = self.base_results.values().cloned().collect::<Vec<_>>();
        cells.sort_by_key(|cell| (cell.sheet, cell.r, cell.c));
        cells
    }

    pub fn unevaluated_formulas(&self) -> u64 {
        self.formula_cells
            .saturating_sub(u64::try_from(self.base_results.len()).unwrap_or(u64::MAX))
    }

    pub fn unevaluated_formulas_on_sheet(&self, sheet: u32) -> u64 {
        let total = usize::try_from(sheet)
            .ok()
            .and_then(|sheet| self.formula_cells_by_sheet.get(sheet))
            .copied()
            .unwrap_or(0);
        let evaluated = self
            .base_results
            .keys()
            .filter(|cell| cell.sheet == sheet)
            .count();
        total.saturating_sub(u64::try_from(evaluated).unwrap_or(u64::MAX))
    }

    pub fn evaluated_cell(&self, cell: CellRef) -> Option<&EvaluatedCell> {
        self.base_results.get(&cell)
    }

    pub fn apply_to_window(
        &self,
        sheet: u32,
        r0: u32,
        c0: u32,
        rows: &mut [Vec<Option<WindowCell>>],
    ) {
        for (row_offset, row) in rows.iter_mut().enumerate() {
            for (column_offset, cell) in row.iter_mut().enumerate() {
                let Some(cell) = cell.as_mut() else {
                    continue;
                };
                let reference = CellRef {
                    sheet,
                    r: r0.saturating_add(u32::try_from(row_offset).unwrap_or(u32::MAX)),
                    c: c0.saturating_add(u32::try_from(column_offset).unwrap_or(u32::MAX)),
                };
                if let Some(evaluated) = self.base_results.get(&reference) {
                    evaluated.apply(cell);
                }
            }
        }
    }

    pub fn recalculate(
        &self,
        store: &WorkbookStore,
        overrides: &[CellOverride],
        budget: Duration,
    ) -> Result<RecalcOutcome, EvalError> {
        let overrides = collapse_overrides(store, overrides)?;
        let touched = overrides.keys().copied().collect::<HashSet<_>>();
        let override_rows = override_row_index(touched.iter().copied());
        let literal_overrides = overrides
            .iter()
            .filter(|(_, entry)| entry.f.is_none())
            .map(|(cell, entry)| {
                (
                    *cell,
                    entry.v.as_ref().map_or(Scalar::Blank, Scalar::from_value),
                )
            })
            .collect::<HashMap<_, _>>();

        // Recalc is an immutable request-local layer. Retain every covered
        // file formula that was not overridden, then add authored formulas
        // at their target coordinates. Rebuilding these graph indexes is
        // what lets a new formula participate in dependencies in both
        // directions without mutating the handle's store or base engine.
        let mut nodes = self
            .nodes
            .iter()
            .filter(|node| !touched.contains(&node.cell))
            .cloned()
            .collect::<Vec<_>>();
        let sheet_names = (0..store.sheet_count())
            .filter_map(|sheet| {
                store
                    .sheet_meta(sheet)
                    .map(|meta| (meta.name.to_ascii_lowercase(), sheet))
            })
            .collect::<HashMap<_, _>>();
        let mut authored = overrides
            .iter()
            .filter_map(|(cell, entry)| entry.f.as_deref().map(|formula| (*cell, formula)))
            .collect::<Vec<_>>();
        authored.sort_by_key(|(cell, _)| *cell);
        for (cell, formula) in authored {
            let (ast, deps) = match parse_formula(formula, cell.sheet, &sheet_names) {
                Ok(ast) => {
                    let mut deps = Vec::new();
                    ast.dependencies(&mut deps);
                    (ast, deps)
                }
                Err(ParseError::Unsupported) => (Expr::Error("#NAME?".to_owned()), Vec::new()),
                Err(ParseError::Invalid | ParseError::TooLarge) => {
                    (Expr::Error("#VALUE!".to_owned()), Vec::new())
                }
            };
            let stored = store.cell(cell.sheet, cell.r, cell.c);
            nodes.push(FormulaNode {
                cell,
                ast,
                deps,
                fmt: stored.as_ref().and_then(|cell| cell.fmt.clone()),
                cached_type: stored.as_ref().map_or(CellType::N, |cell| cell.t),
                authored: true,
            });
        }

        let node_by_cell = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.cell, index))
            .collect::<HashMap<_, _>>();
        let formula_rows = formula_row_index(&nodes);
        let mut dependencies = vec![Vec::new(); nodes.len()];
        let mut reverse = vec![Vec::new(); nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            let mut formula_dependencies = HashSet::new();
            for dependency in &node.deps {
                match dependency {
                    Dependency::Cell(cell) => {
                        if let Some(&other) = node_by_cell.get(cell) {
                            formula_dependencies.insert(other);
                        }
                    }
                    Dependency::Range(range) => collect_formula_nodes_in_range(
                        &formula_rows,
                        *range,
                        &mut formula_dependencies,
                    ),
                }
            }
            dependencies[index].extend(formula_dependencies);
            dependencies[index].sort_unstable();
            for &dependency in &dependencies[index] {
                reverse[dependency].push(index);
            }
        }
        for dependents in &mut reverse {
            dependents.sort_unstable();
            dependents.dedup();
        }
        let (topo, cycle_or_downstream) = topological_order(&dependencies, &reverse);

        let mut dirty = HashSet::new();
        for (index, node) in nodes.iter().enumerate() {
            if node.authored
                || node
                    .deps
                    .iter()
                    .any(|dependency| dependency_touched(*dependency, &touched, &override_rows))
            {
                dirty.insert(index);
            }
        }
        let mut queue = dirty.iter().copied().collect::<VecDeque<_>>();
        while let Some(node) = queue.pop_front() {
            for &dependent in &reverse[node] {
                if dirty.insert(dependent) {
                    queue.push_back(dependent);
                }
            }
        }

        let started = Instant::now();
        let deadline = started.checked_add(budget).unwrap_or(started);
        let mut results = self.base_results.clone();
        for cell in overrides.keys() {
            results.remove(cell);
        }
        let mut evaluated = 0_u64;
        let mut budget_exceeded = false;
        for &index in &topo {
            if !dirty.contains(&index) {
                continue;
            }
            if Instant::now() >= deadline {
                budget_exceeded = true;
                break;
            }
            let node = &nodes[index];
            let mut context = EvalContext {
                store,
                nodes: &node_by_cell,
                results: &results,
                overrides: &literal_overrides,
                deadline,
            };
            match context.eval(&node.ast) {
                Ok(value) => {
                    results.insert(node.cell, evaluated_cell(node, value, store.date_1904()));
                    evaluated = evaluated.saturating_add(1);
                }
                Err(EvalFailure::Budget) => {
                    budget_exceeded = true;
                    break;
                }
            }
        }

        // Residual graph nodes are cycle members or downstream of a cycle.
        // If an override did not break the relevant chain, they remain loud.
        for &index in &cycle_or_downstream {
            if !dirty.contains(&index) {
                continue;
            }
            if Instant::now() >= deadline {
                budget_exceeded = true;
                break;
            }
            let node = &nodes[index];
            results.insert(node.cell, error_cell(node.cell, "#CYCLE!"));
            evaluated = evaluated.saturating_add(1);
        }

        let skipped = u64::try_from(dirty.len())
            .unwrap_or(u64::MAX)
            .saturating_sub(evaluated);
        let mut warnings = Vec::new();
        if budget_exceeded {
            warnings.push(format!(
                "formula evaluation budget exceeded after {} ms; {skipped} dirty formula cells kept their previous values",
                budget.as_millis()
            ));
        }

        let mut changed = dirty
            .iter()
            .filter_map(|&index| {
                let node = &nodes[index];
                let cell = node.cell;
                let next = results.get(&cell)?;
                (node.authored || self.base_results.get(&cell) != Some(next)).then(|| next.clone())
            })
            .collect::<Vec<_>>();
        changed.extend(
            overrides
                .iter()
                .filter(|(_, entry)| entry.f.is_none())
                .map(|(cell, entry)| literal_override_cell(store, *cell, entry.v.as_ref())),
        );
        changed.sort_by_key(|cell| (cell.sheet, cell.r, cell.c));
        let truncated = bound_changed_set(&mut changed, &mut warnings);
        // Export needs only formulas dirtied by this override layer. Keeping
        // unrelated file caches byte-stable preserves no-edit round trips;
        // every dirty covered downstream formula still receives a fresh
        // cache, even if its value happens not to have changed.
        let mut all_evaluated = dirty
            .iter()
            .filter_map(|&index| results.get(&nodes[index].cell).cloned())
            .collect::<Vec<_>>();
        all_evaluated.sort_by_key(|cell| (cell.sheet, cell.r, cell.c));
        Ok(RecalcOutcome {
            changed,
            all_evaluated,
            evaluated,
            skipped,
            truncated,
            warnings,
        })
    }

    fn evaluate_all(
        &self,
        store: &WorkbookStore,
        budget: Duration,
    ) -> (HashMap<CellRef, EvaluatedCell>, EvaluationStats) {
        let started = Instant::now();
        let deadline = started.checked_add(budget).unwrap_or(started);
        let overrides = HashMap::new();
        let mut results = HashMap::new();
        let mut evaluated = 0_u64;
        let mut budget_exceeded = false;
        for &index in &self.cycle_or_downstream {
            if Instant::now() >= deadline {
                budget_exceeded = true;
                break;
            }
            let node = &self.nodes[index];
            results.insert(node.cell, error_cell(node.cell, "#CYCLE!"));
            evaluated = evaluated.saturating_add(1);
        }
        for &index in &self.topo {
            if Instant::now() >= deadline {
                budget_exceeded = true;
                break;
            }
            let node = &self.nodes[index];
            let mut context = EvalContext {
                store,
                nodes: &self.node_by_cell,
                results: &results,
                overrides: &overrides,
                deadline,
            };
            match context.eval(&node.ast) {
                Ok(value) => {
                    results.insert(node.cell, evaluated_cell(node, value, store.date_1904()));
                    evaluated = evaluated.saturating_add(1);
                }
                Err(EvalFailure::Budget) => {
                    budget_exceeded = true;
                    break;
                }
            }
        }
        let skipped = self.formula_cells.saturating_sub(evaluated);
        let mut warnings = Vec::new();
        if budget_exceeded {
            warnings.push(format!(
                "formula evaluation budget exceeded after {} ms; {skipped} formula cells kept file-cached values",
                budget.as_millis()
            ));
        }
        (
            results,
            EvaluationStats {
                evaluated,
                skipped,
                warnings,
            },
        )
    }
}

fn formula_row_index(nodes: &[FormulaNode]) -> FormulaRowIndex {
    let mut index = FormulaRowIndex::new();
    for (node_index, node) in nodes.iter().enumerate() {
        index
            .entry(node.cell.sheet)
            .or_default()
            .entry(node.cell.r)
            .or_default()
            .push((node.cell.c, node_index));
    }
    for rows in index.values_mut() {
        for columns in rows.values_mut() {
            columns.sort_unstable();
        }
    }
    index
}

fn collect_formula_nodes_in_range(
    index: &FormulaRowIndex,
    range: RangeRef,
    output: &mut HashSet<usize>,
) {
    let Some(rows) = index.get(&range.sheet) else {
        return;
    };
    for columns in rows.range(range.r0..=range.r1).map(|(_, columns)| columns) {
        let start = columns.partition_point(|(column, _)| *column < range.c0);
        for &(column, node) in &columns[start..] {
            if column > range.c1 {
                break;
            }
            output.insert(node);
        }
    }
}

fn topological_order(
    dependencies: &[Vec<usize>],
    reverse: &[Vec<usize>],
) -> (Vec<usize>, HashSet<usize>) {
    let mut indegree = dependencies.iter().map(Vec::len).collect::<Vec<_>>();
    let mut queue = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<VecDeque<_>>();
    let mut order = Vec::with_capacity(dependencies.len());
    while let Some(node) = queue.pop_front() {
        order.push(node);
        for &dependent in &reverse[node] {
            indegree[dependent] = indegree[dependent].saturating_sub(1);
            if indegree[dependent] == 0 {
                queue.push_back(dependent);
            }
        }
    }
    let residual = indegree
        .into_iter()
        .enumerate()
        .filter_map(|(index, count)| (count > 0).then_some(index))
        .collect();
    (order, residual)
}

fn override_row_index(
    cells: impl Iterator<Item = CellRef>,
) -> HashMap<u32, BTreeMap<u32, Vec<u32>>> {
    let mut index = HashMap::<u32, BTreeMap<u32, Vec<u32>>>::new();
    for cell in cells {
        index
            .entry(cell.sheet)
            .or_default()
            .entry(cell.r)
            .or_default()
            .push(cell.c);
    }
    for rows in index.values_mut() {
        for columns in rows.values_mut() {
            columns.sort_unstable();
            columns.dedup();
        }
    }
    index
}

fn dependency_touched(
    dependency: Dependency,
    overrides: &HashSet<CellRef>,
    rows: &HashMap<u32, BTreeMap<u32, Vec<u32>>>,
) -> bool {
    match dependency {
        Dependency::Cell(cell) => overrides.contains(&cell),
        Dependency::Range(range) => rows.get(&range.sheet).is_some_and(|sheet_rows| {
            sheet_rows.range(range.r0..=range.r1).any(|(_, columns)| {
                let first = columns.partition_point(|column| *column < range.c0);
                columns.get(first).is_some_and(|column| *column <= range.c1)
            })
        }),
    }
}

fn collapse_overrides(
    store: &WorkbookStore,
    overrides: &[CellOverride],
) -> Result<HashMap<CellRef, CellOverride>, EvalError> {
    if overrides.len() > EXPORT_OVERRIDES_CAP {
        return Err(EvalError::bad_request(format!(
            "overrides length {} exceeds the {EXPORT_OVERRIDES_CAP}-entry cap",
            overrides.len()
        )));
    }
    let mut collapsed = HashMap::new();
    let mut extents = HashMap::<u32, (u32, u32)>::new();
    for (index, entry) in overrides.iter().enumerate() {
        let Some(meta) = store.sheet_meta(entry.sheet) else {
            return Err(EvalError::bad_request(format!(
                "overrides[{index}] sheet index {} is out of range",
                entry.sheet
            )));
        };
        if let Some(CellValue::Number(number)) = &entry.v {
            if !number.is_finite() {
                return Err(EvalError::bad_request(format!(
                    "overrides[{index}].v must be a finite number"
                )));
            }
        }
        let extent = extents.entry(entry.sheet).or_insert((meta.rows, meta.cols));
        extent.0 = extent.0.max(entry.r.saturating_add(1));
        extent.1 = extent.1.max(entry.c.saturating_add(1));
        collapsed.insert(
            CellRef {
                sheet: entry.sheet,
                r: entry.r,
                c: entry.c,
            },
            entry.clone(),
        );
    }
    for (sheet, (rows, cols)) in extents {
        let meta = store.sheet_meta(sheet).expect("validated sheet exists");
        let original = u64::from(meta.rows) * u64::from(meta.cols);
        let extended = u64::from(rows) * u64::from(cols);
        if extended > OVERRIDE_EXTENT_CAP_CELLS && extended > original {
            return Err(EvalError::bomb(format!(
                "overrides extend sheet {:?} to {rows}x{cols} ({extended} cells), exceeding the {OVERRIDE_EXTENT_CAP_CELLS} cell limit",
                meta.name
            )));
        }
    }
    Ok(collapsed)
}

#[derive(Clone, Debug, PartialEq)]
enum Scalar {
    Blank,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(String),
}

impl Scalar {
    fn from_value(value: &CellValue) -> Self {
        match value {
            CellValue::Number(value) => Self::Number(*value),
            CellValue::Text(value) => Self::Text(value.clone()),
            CellValue::Bool(value) => Self::Bool(*value),
        }
    }

    fn from_cell(cell: &WindowCell, date_1904: bool) -> Self {
        if cell.t == CellType::E {
            return Self::Error(match &cell.v {
                Some(CellValue::Text(error)) => error.clone(),
                _ => "#VALUE!".to_owned(),
            });
        }
        if cell.t == CellType::D {
            if let Some(CellValue::Text(value)) = &cell.v {
                if let Some(serial) = iso_to_excel_serial(value, date_1904) {
                    return Self::Number(serial);
                }
            }
        }
        match &cell.v {
            Some(value) => Self::from_value(value),
            None => Self::Blank,
        }
    }

    fn from_evaluated(cell: &EvaluatedCell, date_1904: bool) -> Self {
        if cell.t == CellType::E {
            return Self::Error(match &cell.v {
                Some(CellValue::Text(error)) => error.clone(),
                _ => "#VALUE!".to_owned(),
            });
        }
        if cell.t == CellType::D {
            if let Some(CellValue::Text(value)) = &cell.v {
                if let Some(serial) = iso_to_excel_serial(value, date_1904) {
                    return Self::Number(serial);
                }
            }
        }
        match &cell.v {
            Some(value) => Self::from_value(value),
            None => Self::Blank,
        }
    }

    fn number(&self) -> Result<f64, Scalar> {
        match self {
            Self::Number(value) => Ok(*value),
            Self::Bool(value) => Ok(u8::from(*value).into()),
            Self::Blank => Ok(0.0),
            Self::Text(value) => value.trim().parse::<f64>().map_err(|_| value_error()),
            Self::Error(error) => Err(Self::Error(error.clone())),
        }
    }

    fn boolean(&self) -> Result<bool, Scalar> {
        match self {
            Self::Bool(value) => Ok(*value),
            Self::Number(value) => Ok(*value != 0.0),
            Self::Blank => Ok(false),
            Self::Text(value) if value.eq_ignore_ascii_case("TRUE") => Ok(true),
            Self::Text(value) if value.eq_ignore_ascii_case("FALSE") => Ok(false),
            Self::Text(_) => Err(value_error()),
            Self::Error(error) => Err(Self::Error(error.clone())),
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Blank => String::new(),
            Self::Number(value) => general_number(*value),
            Self::Text(value) | Self::Error(value) => value.clone(),
            Self::Bool(true) => "TRUE".to_owned(),
            Self::Bool(false) => "FALSE".to_owned(),
        }
    }
}

fn general_number(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}

fn iso_to_excel_serial(value: &str, date_1904: bool) -> Option<f64> {
    // chrono correctly rejects 1900-02-29, but the fictitious day is part of
    // Excel's 1900 date system and is emitted by the reader for serial 60.
    if !date_1904 {
        if value == "1900-02-29" {
            return Some(60.0);
        }
        if let Some(time) = value.strip_prefix("1900-02-29T") {
            let surrogate = format!("1970-01-01T{time}");
            let datetime =
                NaiveDateTime::parse_from_str(&surrogate, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
            let midnight = datetime.date().and_hms_opt(0, 0, 0)?;
            let fraction =
                datetime.signed_duration_since(midnight).num_milliseconds() as f64 / 86_400_000.0;
            return Some(60.0 + fraction);
        }
    }
    let datetime = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
        })?;
    let date = datetime.date();
    let base = if date_1904 {
        NaiveDate::from_ymd_opt(1904, 1, 1)?
    } else if date < NaiveDate::from_ymd_opt(1900, 3, 1)? {
        NaiveDate::from_ymd_opt(1899, 12, 31)?
    } else {
        // Excel's fictitious 1900-02-29 occupies serial 60.
        NaiveDate::from_ymd_opt(1899, 12, 30)?
    };
    let midnight = date.and_hms_opt(0, 0, 0)?;
    let days = date.signed_duration_since(base).num_days() as f64;
    let fraction =
        datetime.signed_duration_since(midnight).num_milliseconds() as f64 / 86_400_000.0;
    Some(days + fraction)
}

fn excel_serial_iso(serial: f64, date_1904: bool) -> String {
    let value = ExcelDateTime::new(serial, ExcelDateTimeType::DateTime, date_1904);
    let (year, month, day, hour, minute, second, millisecond) = value.to_ymd_hms_milli();
    if hour == 0 && minute == 0 && second == 0 && millisecond == 0 {
        format!("{year:04}-{month:02}-{day:02}")
    } else if millisecond == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}")
    }
}

fn value_error() -> Scalar {
    Scalar::Error("#VALUE!".to_owned())
}

fn num_error() -> Scalar {
    Scalar::Error("#NUM!".to_owned())
}

fn div_zero_error() -> Scalar {
    Scalar::Error("#DIV/0!".to_owned())
}

fn literal_override_cell(
    store: &WorkbookStore,
    cell: CellRef,
    value: Option<&CellValue>,
) -> EvaluatedCell {
    let fmt = store
        .cell(cell.sheet, cell.r, cell.c)
        .and_then(|stored| stored.fmt);
    let (t, d) = match value {
        None => (CellType::S, None),
        Some(CellValue::Number(number)) => (
            CellType::N,
            fmt.as_deref().and_then(|code| {
                wax_fmt::render(code, FmtValue::Number(*number), store.date_1904())
            }),
        ),
        Some(CellValue::Text(text)) => (
            CellType::S,
            fmt.as_deref()
                .and_then(|code| wax_fmt::render(code, FmtValue::Text(text), store.date_1904())),
        ),
        Some(CellValue::Bool(boolean)) => (
            CellType::B,
            fmt.as_deref().and_then(|code| {
                wax_fmt::render(code, FmtValue::Bool(*boolean), store.date_1904())
            }),
        ),
    };
    EvaluatedCell {
        sheet: cell.sheet,
        r: cell.r,
        c: cell.c,
        t,
        v: value.cloned(),
        d,
        e: false,
    }
}

fn formula_display(
    node: &FormulaNode,
    value: FmtValue<'_>,
    date_1904: bool,
    general: impl FnOnce() -> String,
) -> Option<String> {
    match node.fmt.as_deref() {
        Some(code) => wax_fmt::render(code, value, date_1904),
        None => node.authored.then(general),
    }
}

fn evaluated_cell(node: &FormulaNode, value: Scalar, date_1904: bool) -> EvaluatedCell {
    let (t, v, d) = match value {
        Scalar::Blank => {
            let d = formula_display(node, FmtValue::Number(0.0), date_1904, || "0".to_owned());
            if node.cached_type == CellType::D {
                (
                    CellType::D,
                    Some(CellValue::Text(excel_serial_iso(0.0, date_1904))),
                    d,
                )
            } else {
                (CellType::N, Some(CellValue::Number(0.0)), d)
            }
        }
        Scalar::Number(number) => {
            let d = formula_display(node, FmtValue::Number(number), date_1904, || {
                general_number(number)
            });
            if node.cached_type == CellType::D && number >= 0.0 {
                (
                    CellType::D,
                    Some(CellValue::Text(excel_serial_iso(number, date_1904))),
                    d,
                )
            } else {
                (CellType::N, Some(CellValue::Number(number)), d)
            }
        }
        Scalar::Text(text) => {
            let d = formula_display(node, FmtValue::Text(&text), date_1904, || text.clone());
            (CellType::S, Some(CellValue::Text(text)), d)
        }
        Scalar::Bool(boolean) => {
            let d = formula_display(node, FmtValue::Bool(boolean), date_1904, || {
                if boolean { "TRUE" } else { "FALSE" }.to_owned()
            });
            (CellType::B, Some(CellValue::Bool(boolean)), d)
        }
        Scalar::Error(error) => (
            CellType::E,
            Some(CellValue::Text(error.clone())),
            Some(error),
        ),
    };
    EvaluatedCell {
        sheet: node.cell.sheet,
        r: node.cell.r,
        c: node.cell.c,
        t,
        v,
        d,
        e: true,
    }
}

fn error_cell(cell: CellRef, error: &str) -> EvaluatedCell {
    EvaluatedCell {
        sheet: cell.sheet,
        r: cell.r,
        c: cell.c,
        t: CellType::E,
        v: Some(CellValue::Text(error.to_owned())),
        d: Some(error.to_owned()),
        e: true,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RangeRef {
    sheet: u32,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
}

impl RangeRef {
    fn new(first: CellRef, last: CellRef) -> Result<Self, ParseError> {
        if first.sheet != last.sheet {
            return Err(ParseError::Unsupported);
        }
        Ok(Self {
            sheet: first.sheet,
            r0: first.r.min(last.r),
            c0: first.c.min(last.c),
            r1: first.r.max(last.r),
            c1: first.c.max(last.c),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Dependency {
    Cell(CellRef),
    Range(RangeRef),
}

#[derive(Clone, Debug)]
enum Expr {
    Blank,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(String),
    Ref(CellRef),
    Range(RangeRef),
    Unary(UnaryOp, Box<Self>),
    Binary(BinaryOp, Box<Self>, Box<Self>),
    Function(Function, Vec<Self>),
}

impl Expr {
    fn dependencies(&self, output: &mut Vec<Dependency>) {
        match self {
            Self::Ref(cell) => output.push(Dependency::Cell(*cell)),
            Self::Range(range) => output.push(Dependency::Range(*range)),
            Self::Unary(_, expression) => expression.dependencies(output),
            Self::Binary(_, left, right) => {
                left.dependencies(output);
                right.dependencies(output);
            }
            Self::Function(_, arguments) => {
                for argument in arguments {
                    argument.dependencies(output);
                }
            }
            Self::Blank | Self::Number(_) | Self::Text(_) | Self::Bool(_) | Self::Error(_) => {}
        }
    }

    /// The MVP supports ranges as arguments to functions that explicitly
    /// consume lists. Legacy implicit intersection and dynamic-array
    /// arithmetic are outside the curated scalar surface and remain cached.
    fn scalar_surface(&self) -> bool {
        match self {
            Self::Blank
            | Self::Number(_)
            | Self::Text(_)
            | Self::Bool(_)
            | Self::Error(_)
            | Self::Ref(_) => true,
            Self::Range(_) => false,
            Self::Unary(_, expression) => expression.scalar_surface(),
            Self::Binary(_, left, right) => left.scalar_surface() && right.scalar_surface(),
            Self::Function(function, arguments) => match function {
                Function::Sum
                | Function::Average
                | Function::Count
                | Function::CountA
                | Function::Min
                | Function::Max
                | Function::And
                | Function::Or
                | Function::Concat => arguments.iter().all(|argument| {
                    matches!(argument, Self::Range(_)) || argument.scalar_surface()
                }),
                Function::If | Function::Not | Function::Round | Function::Abs => {
                    arguments.iter().all(Self::scalar_surface)
                }
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum UnaryOp {
    Plus,
    Minus,
    Percent,
}

#[derive(Clone, Copy, Debug)]
enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Concat,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Function {
    Sum,
    Average,
    Count,
    CountA,
    Min,
    Max,
    If,
    And,
    Or,
    Not,
    Round,
    Abs,
    Concat,
}

impl Function {
    fn parse(name: &str) -> Option<Self> {
        let name = name
            .strip_prefix("_xlfn.")
            .or_else(|| name.strip_prefix("_XLFN."))
            .unwrap_or(name);
        Some(match name.to_ascii_uppercase().as_str() {
            "SUM" => Self::Sum,
            "AVERAGE" => Self::Average,
            "COUNT" => Self::Count,
            "COUNTA" => Self::CountA,
            "MIN" => Self::Min,
            "MAX" => Self::Max,
            "IF" => Self::If,
            "AND" => Self::And,
            "OR" => Self::Or,
            "NOT" => Self::Not,
            "ROUND" => Self::Round,
            "ABS" => Self::Abs,
            "CONCAT" => Self::Concat,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    String(String),
    Word(String),
    QuotedSheet(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Percent,
    Ampersand,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    LParen,
    RParen,
    Comma,
    Colon,
    Bang,
    Eof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParseError {
    Invalid,
    Unsupported,
    TooLarge,
}

fn parse_formula(
    formula: &str,
    current_sheet: u32,
    sheet_names: &HashMap<String, u32>,
) -> Result<Expr, ParseError> {
    if formula.len() > MAX_FORMULA_BYTES {
        return Err(ParseError::TooLarge);
    }
    let mut lexer = Lexer::new(formula.strip_prefix('=').unwrap_or(formula));
    let tokens = lexer.tokenize()?;
    let mut parser = Parser {
        tokens,
        position: 0,
        current_sheet,
        sheet_names,
        nodes: 0,
        depth: 0,
    };
    let expression = parser.comparison()?;
    if parser.peek() != &Token::Eof {
        return Err(ParseError::Unsupported);
    }
    if !expression.scalar_surface() {
        return Err(ParseError::Unsupported);
    }
    Ok(expression)
}

struct Lexer<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
        }
    }

    fn tokenize(&mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_space();
            let Some(byte) = self.peek() else {
                tokens.push(Token::Eof);
                return Ok(tokens);
            };
            let token = match byte {
                b'0'..=b'9' | b'.'
                    if byte != b'.'
                        || self
                            .input
                            .get(self.position + 1)
                            .is_some_and(u8::is_ascii_digit) =>
                {
                    self.number()?
                }
                b'"' => Token::String(self.string()?),
                b'\'' => Token::QuotedSheet(self.quoted_sheet()?),
                b'+' => self.one(Token::Plus),
                b'-' => self.one(Token::Minus),
                b'*' => self.one(Token::Star),
                b'/' => self.one(Token::Slash),
                b'^' => self.one(Token::Caret),
                b'%' => self.one(Token::Percent),
                b'&' => self.one(Token::Ampersand),
                b'=' => self.one(Token::Equal),
                b'(' => self.one(Token::LParen),
                b')' => self.one(Token::RParen),
                b',' => self.one(Token::Comma),
                b':' => self.one(Token::Colon),
                b'!' => self.one(Token::Bang),
                b'<' => {
                    self.position += 1;
                    match self.peek() {
                        Some(b'=') => {
                            self.position += 1;
                            Token::LessEqual
                        }
                        Some(b'>') => {
                            self.position += 1;
                            Token::NotEqual
                        }
                        _ => Token::Less,
                    }
                }
                b'>' => {
                    self.position += 1;
                    if self.peek() == Some(b'=') {
                        self.position += 1;
                        Token::GreaterEqual
                    } else {
                        Token::Greater
                    }
                }
                _ if word_byte(byte) => Token::Word(self.word()?),
                _ => return Err(ParseError::Unsupported),
            };
            tokens.push(token);
            if tokens.len() > MAX_AST_NODES.saturating_mul(4) {
                return Err(ParseError::TooLarge);
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn one(&mut self, token: Token) -> Token {
        self.position += 1;
        token
    }

    fn number(&mut self) -> Result<Token, ParseError> {
        let start = self.position;
        let mut saw_digit = false;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            saw_digit = true;
            self.position += 1;
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                saw_digit = true;
                self.position += 1;
            }
        }
        if !saw_digit {
            return Err(ParseError::Invalid);
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            let exponent = self.position;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            if exponent == self.position {
                return Err(ParseError::Invalid);
            }
        }
        let text = std::str::from_utf8(&self.input[start..self.position])
            .map_err(|_| ParseError::Invalid)?;
        let number = text.parse::<f64>().map_err(|_| ParseError::Invalid)?;
        if !number.is_finite() {
            return Err(ParseError::Invalid);
        }
        Ok(Token::Number(number))
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.position += 1;
        let mut output = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(ParseError::Invalid);
            };
            self.position += 1;
            if byte == b'"' {
                if self.peek() == Some(b'"') {
                    output.push(b'"');
                    self.position += 1;
                } else {
                    break;
                }
            } else {
                output.push(byte);
            }
        }
        String::from_utf8(output).map_err(|_| ParseError::Invalid)
    }

    fn quoted_sheet(&mut self) -> Result<String, ParseError> {
        self.position += 1;
        let mut output = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(ParseError::Invalid);
            };
            self.position += 1;
            if byte == b'\'' {
                if self.peek() == Some(b'\'') {
                    output.push(b'\'');
                    self.position += 1;
                } else {
                    break;
                }
            } else {
                output.push(byte);
            }
        }
        String::from_utf8(output).map_err(|_| ParseError::Invalid)
    }

    fn word(&mut self) -> Result<String, ParseError> {
        let start = self.position;
        while self.peek().is_some_and(word_byte) {
            self.position += 1;
        }
        std::str::from_utf8(&self.input[start..self.position])
            .map(str::to_owned)
            .map_err(|_| ParseError::Invalid)
    }
}

fn word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$')
}

struct Parser<'a> {
    tokens: Vec<Token>,
    position: usize,
    current_sheet: u32,
    sheet_names: &'a HashMap<String, u32>,
    nodes: usize,
    depth: usize,
}

impl Parser<'_> {
    fn comparison(&mut self) -> Result<Expr, ParseError> {
        let mut expression = self.concat()?;
        loop {
            let op = match self.peek() {
                Token::Equal => BinaryOp::Equal,
                Token::NotEqual => BinaryOp::NotEqual,
                Token::Less => BinaryOp::Less,
                Token::LessEqual => BinaryOp::LessEqual,
                Token::Greater => BinaryOp::Greater,
                Token::GreaterEqual => BinaryOp::GreaterEqual,
                _ => break,
            };
            self.position += 1;
            let right = self.concat()?;
            expression = self.node(Expr::Binary(op, Box::new(expression), Box::new(right)))?;
        }
        Ok(expression)
    }

    fn concat(&mut self) -> Result<Expr, ParseError> {
        let mut expression = self.add_subtract()?;
        while self.consume(&Token::Ampersand) {
            let right = self.add_subtract()?;
            expression = self.node(Expr::Binary(
                BinaryOp::Concat,
                Box::new(expression),
                Box::new(right),
            ))?;
        }
        Ok(expression)
    }

    fn add_subtract(&mut self) -> Result<Expr, ParseError> {
        let mut expression = self.multiply_divide()?;
        loop {
            let op = if self.consume(&Token::Plus) {
                BinaryOp::Add
            } else if self.consume(&Token::Minus) {
                BinaryOp::Subtract
            } else {
                break;
            };
            let right = self.multiply_divide()?;
            expression = self.node(Expr::Binary(op, Box::new(expression), Box::new(right)))?;
        }
        Ok(expression)
    }

    fn multiply_divide(&mut self) -> Result<Expr, ParseError> {
        let mut expression = self.power()?;
        loop {
            let op = if self.consume(&Token::Star) {
                BinaryOp::Multiply
            } else if self.consume(&Token::Slash) {
                BinaryOp::Divide
            } else {
                break;
            };
            let right = self.power()?;
            expression = self.node(Expr::Binary(op, Box::new(expression), Box::new(right)))?;
        }
        Ok(expression)
    }

    fn power(&mut self) -> Result<Expr, ParseError> {
        let expression = self.unary()?;
        if self.consume(&Token::Caret) {
            let right = self.nested(Self::power)?;
            Ok(self.node(Expr::Binary(
                BinaryOp::Power,
                Box::new(expression),
                Box::new(right),
            ))?)
        } else {
            Ok(expression)
        }
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        if self.consume(&Token::Plus) {
            return self
                .nested(Self::unary)
                .and_then(|value| self.node(Expr::Unary(UnaryOp::Plus, Box::new(value))));
        }
        if self.consume(&Token::Minus) {
            return self
                .nested(Self::unary)
                .and_then(|value| self.node(Expr::Unary(UnaryOp::Minus, Box::new(value))));
        }
        let mut expression = self.primary()?;
        while self.consume(&Token::Percent) {
            expression = self.node(Expr::Unary(UnaryOp::Percent, Box::new(expression)))?;
        }
        Ok(expression)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let token = self.take().clone();
        let expression = match token {
            Token::Number(number) => Expr::Number(number),
            Token::String(text) => Expr::Text(text),
            Token::LParen => {
                let expression = self.nested(Self::comparison)?;
                self.expect(Token::RParen)?;
                expression
            }
            Token::Word(word) | Token::QuotedSheet(word) => {
                if self.consume(&Token::LParen) {
                    let function = Function::parse(&word).ok_or(ParseError::Unsupported)?;
                    let arguments = self.arguments()?;
                    Expr::Function(function, arguments)
                } else if self.consume(&Token::Bang) {
                    let sheet = self
                        .sheet_names
                        .get(&word.to_ascii_lowercase())
                        .copied()
                        .ok_or(ParseError::Unsupported)?;
                    let first = self.reference_token(sheet)?;
                    if self.consume(&Token::Colon) {
                        let last = self.reference_token(sheet)?;
                        Expr::Range(RangeRef::new(first, last)?)
                    } else {
                        Expr::Ref(first)
                    }
                } else if word.eq_ignore_ascii_case("TRUE") {
                    Expr::Bool(true)
                } else if word.eq_ignore_ascii_case("FALSE") {
                    Expr::Bool(false)
                } else if let Some(first) = parse_a1(&word, self.current_sheet) {
                    if self.consume(&Token::Colon) {
                        let last = self.reference_token(self.current_sheet)?;
                        Expr::Range(RangeRef::new(first, last)?)
                    } else {
                        Expr::Ref(first)
                    }
                } else {
                    return Err(ParseError::Unsupported);
                }
            }
            _ => return Err(ParseError::Invalid),
        };
        self.node(expression)
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut arguments = Vec::new();
        if self.consume(&Token::RParen) {
            return Ok(arguments);
        }
        loop {
            if self.consume(&Token::Comma) {
                arguments.push(Expr::Blank);
                continue;
            }
            arguments.push(self.nested(Self::comparison)?);
            if self.consume(&Token::RParen) {
                break;
            }
            self.expect(Token::Comma)?;
            if self.consume(&Token::RParen) {
                arguments.push(Expr::Blank);
                break;
            }
        }
        Ok(arguments)
    }

    fn reference_token(&mut self, sheet: u32) -> Result<CellRef, ParseError> {
        let Token::Word(word) = self.take().clone() else {
            return Err(ParseError::Invalid);
        };
        parse_a1(&word, sheet).ok_or(ParseError::Invalid)
    }

    fn node(&mut self, expression: Expr) -> Result<Expr, ParseError> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAX_AST_NODES {
            Err(ParseError::TooLarge)
        } else {
            Ok(expression)
        }
    }

    fn nested<T>(
        &mut self,
        parse: fn(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(ParseError::TooLarge);
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.position).unwrap_or(&Token::Eof)
    }

    fn take(&mut self) -> &Token {
        let index = self.position;
        self.position = self.position.saturating_add(1);
        self.tokens.get(index).unwrap_or(&Token::Eof)
    }

    fn consume(&mut self, expected: &Token) -> bool {
        if self.peek() == expected {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: Token) -> Result<(), ParseError> {
        if self.consume(&expected) {
            Ok(())
        } else {
            Err(ParseError::Invalid)
        }
    }
}

fn parse_a1(reference: &str, sheet: u32) -> Option<CellRef> {
    let reference = reference.as_bytes();
    let mut index = usize::from(reference.first() == Some(&b'$'));
    let start_column = index;
    while reference
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphabetic())
    {
        index += 1;
    }
    if index == start_column {
        return None;
    }
    let mut column = 0_u32;
    for byte in &reference[start_column..index] {
        column = column
            .checked_mul(26)?
            .checked_add(u32::from(byte.to_ascii_uppercase() - b'A' + 1))?;
    }
    if reference.get(index) == Some(&b'$') {
        index += 1;
    }
    let row_text = std::str::from_utf8(reference.get(index..)?).ok()?;
    if row_text.is_empty() || !row_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let row = row_text.parse::<u32>().ok()?;
    if row == 0 || column == 0 {
        return None;
    }
    Some(CellRef {
        sheet,
        r: row - 1,
        c: column - 1,
    })
}

enum EvalFailure {
    Budget,
}

struct EvalContext<'a> {
    store: &'a WorkbookStore,
    nodes: &'a HashMap<CellRef, usize>,
    results: &'a HashMap<CellRef, EvaluatedCell>,
    overrides: &'a HashMap<CellRef, Scalar>,
    deadline: Instant,
}

impl EvalContext<'_> {
    fn check(&self) -> Result<(), EvalFailure> {
        if Instant::now() >= self.deadline {
            Err(EvalFailure::Budget)
        } else {
            Ok(())
        }
    }

    fn eval(&mut self, expression: &Expr) -> Result<Scalar, EvalFailure> {
        self.check()?;
        Ok(match expression {
            Expr::Blank => Scalar::Blank,
            Expr::Number(number) => Scalar::Number(*number),
            Expr::Text(text) => Scalar::Text(text.clone()),
            Expr::Bool(boolean) => Scalar::Bool(*boolean),
            Expr::Error(error) => Scalar::Error(error.clone()),
            Expr::Ref(reference) => self.cell(*reference),
            Expr::Range(_) => value_error(),
            Expr::Unary(operator, expression) => {
                let value = self.eval(expression)?;
                match operator {
                    // Excel/Calc retain a formula-produced empty string under
                    // unary plus; treating it as an unparseable numeric string
                    // would manufacture #VALUE! for the common `+A1` idiom.
                    UnaryOp::Plus if matches!(&value, Scalar::Text(text) if text.is_empty()) => {
                        value
                    }
                    UnaryOp::Plus => value.number().map(Scalar::Number).unwrap_or_else(|e| e),
                    UnaryOp::Minus => value
                        .number()
                        .map(|number| Scalar::Number(-number))
                        .unwrap_or_else(|e| e),
                    UnaryOp::Percent => value
                        .number()
                        .map(|number| Scalar::Number(number / 100.0))
                        .unwrap_or_else(|e| e),
                }
            }
            Expr::Binary(operator, left, right) => {
                let left = self.eval(left)?;
                let right = self.eval(right)?;
                binary(*operator, left, right)
            }
            Expr::Function(function, arguments) => self.function(*function, arguments)?,
        })
    }

    fn cell(&self, reference: CellRef) -> Scalar {
        if let Some(value) = self.overrides.get(&reference) {
            return value.clone();
        }
        if self.nodes.contains_key(&reference) {
            if let Some(value) = self.results.get(&reference) {
                return Scalar::from_evaluated(value, self.store.date_1904());
            }
        }
        self.store
            .cell(reference.sheet, reference.r, reference.c)
            .as_ref()
            .map_or(Scalar::Blank, |cell| {
                Scalar::from_cell(cell, self.store.date_1904())
            })
    }

    fn function(&mut self, function: Function, arguments: &[Expr]) -> Result<Scalar, EvalFailure> {
        match function {
            Function::If => {
                if !(2..=3).contains(&arguments.len()) {
                    return Ok(value_error());
                }
                let condition = self.eval(&arguments[0])?;
                let condition = match condition.boolean() {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                if condition {
                    self.eval(&arguments[1])
                } else if let Some(value) = arguments.get(2) {
                    self.eval(value)
                } else {
                    Ok(Scalar::Bool(false))
                }
            }
            Function::Not => {
                if arguments.len() != 1 {
                    return Ok(value_error());
                }
                Ok(match self.eval(&arguments[0])?.boolean() {
                    Ok(value) => Scalar::Bool(!value),
                    Err(error) => error,
                })
            }
            Function::Round => {
                if arguments.len() != 2 {
                    return Ok(value_error());
                }
                let value = match self.eval(&arguments[0])?.number() {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                let digits = match self.eval(&arguments[1])?.number() {
                    Ok(value) => value.trunc(),
                    Err(error) => return Ok(error),
                };
                if !(-308.0..=308.0).contains(&digits) {
                    return Ok(num_error());
                }
                let factor = 10_f64.powf(digits.abs());
                let rounded = if digits >= 0.0 {
                    round_excel(value * factor) / factor
                } else {
                    round_excel(value / factor) * factor
                };
                Ok(if rounded.is_finite() {
                    Scalar::Number(rounded)
                } else {
                    num_error()
                })
            }
            Function::Abs => {
                if arguments.len() != 1 {
                    return Ok(value_error());
                }
                Ok(match self.eval(&arguments[0])?.number() {
                    Ok(value) => Scalar::Number(value.abs()),
                    Err(error) => error,
                })
            }
            Function::Concat => {
                let mut output = String::new();
                for argument in arguments {
                    for item in self.items(argument)? {
                        if let Scalar::Error(error) = item.value {
                            return Ok(Scalar::Error(error));
                        }
                        output.push_str(&item.value.text());
                    }
                }
                Ok(Scalar::Text(output))
            }
            Function::And | Function::Or => {
                let mut saw_logical = false;
                let mut result = function == Function::And;
                for argument in arguments {
                    for item in self.items(argument)? {
                        match logical_item(&item) {
                            Ok(Some(value)) => {
                                saw_logical = true;
                                if function == Function::And {
                                    result &= value;
                                } else {
                                    result |= value;
                                }
                            }
                            Ok(None) => {}
                            Err(error) => return Ok(error),
                        }
                    }
                }
                Ok(if saw_logical {
                    Scalar::Bool(result)
                } else {
                    value_error()
                })
            }
            Function::CountA => {
                let mut count = 0_u64;
                for argument in arguments {
                    for item in self.items(argument)? {
                        if !matches!(item.value, Scalar::Blank) {
                            count = count.saturating_add(1);
                        }
                    }
                }
                Ok(Scalar::Number(count as f64))
            }
            Function::Sum | Function::Average | Function::Count | Function::Min | Function::Max => {
                let mut numbers = Vec::new();
                for argument in arguments {
                    for item in self.items(argument)? {
                        if let Scalar::Error(error) = item.value {
                            return Ok(Scalar::Error(error));
                        }
                        if let Some(number) = numeric_item(&item) {
                            numbers.push(number);
                        }
                    }
                }
                Ok(match function {
                    Function::Sum => Scalar::Number(numbers.iter().sum()),
                    Function::Average if numbers.is_empty() => div_zero_error(),
                    Function::Average => {
                        Scalar::Number(numbers.iter().sum::<f64>() / numbers.len() as f64)
                    }
                    Function::Count => Scalar::Number(numbers.len() as f64),
                    Function::Min => {
                        Scalar::Number(numbers.into_iter().reduce(f64::min).unwrap_or_default())
                    }
                    Function::Max => {
                        Scalar::Number(numbers.into_iter().reduce(f64::max).unwrap_or_default())
                    }
                    _ => unreachable!(),
                })
            }
        }
    }

    fn items(&mut self, expression: &Expr) -> Result<Vec<Item>, EvalFailure> {
        match expression {
            Expr::Ref(reference) => Ok(vec![Item {
                value: self.cell(*reference),
                from_reference: true,
            }]),
            Expr::Range(range) => {
                let rows = u64::from(range.r1 - range.r0 + 1);
                let cols = u64::from(range.c1 - range.c0 + 1);
                let capacity = usize::try_from(rows.saturating_mul(cols)).unwrap_or(0);
                // Avoid a large eager allocation for a syntactically valid
                // but enormous range. The vector grows only as evaluation
                // makes progress under the wall-clock budget.
                let mut items = Vec::with_capacity(capacity.min(16_384));
                for row in range.r0..=range.r1 {
                    self.check()?;
                    for column in range.c0..=range.c1 {
                        // A single row may itself span a huge, malformed
                        // coordinate range. Keep the wall-clock rail strict
                        // inside both dimensions rather than only per row.
                        self.check()?;
                        items.push(Item {
                            value: self.cell(CellRef {
                                sheet: range.sheet,
                                r: row,
                                c: column,
                            }),
                            from_reference: true,
                        });
                    }
                }
                Ok(items)
            }
            _ => Ok(vec![Item {
                value: self.eval(expression)?,
                from_reference: false,
            }]),
        }
    }
}

struct Item {
    value: Scalar,
    from_reference: bool,
}

fn numeric_item(item: &Item) -> Option<f64> {
    match &item.value {
        Scalar::Number(value) => Some(*value),
        Scalar::Bool(value) if !item.from_reference => Some(u8::from(*value).into()),
        Scalar::Text(value) if !item.from_reference => value.trim().parse().ok(),
        Scalar::Blank | Scalar::Bool(_) | Scalar::Text(_) | Scalar::Error(_) => None,
    }
}

fn logical_item(item: &Item) -> Result<Option<bool>, Scalar> {
    match &item.value {
        Scalar::Error(error) => Err(Scalar::Error(error.clone())),
        Scalar::Bool(value) => Ok(Some(*value)),
        Scalar::Number(value) => Ok(Some(*value != 0.0)),
        Scalar::Text(_) | Scalar::Blank if item.from_reference => Ok(None),
        Scalar::Text(value) if value.eq_ignore_ascii_case("TRUE") => Ok(Some(true)),
        Scalar::Text(value) if value.eq_ignore_ascii_case("FALSE") => Ok(Some(false)),
        Scalar::Text(_) | Scalar::Blank => Err(value_error()),
    }
}

fn binary(operator: BinaryOp, left: Scalar, right: Scalar) -> Scalar {
    if let Scalar::Error(error) = &left {
        return Scalar::Error(error.clone());
    }
    if let Scalar::Error(error) = &right {
        return Scalar::Error(error.clone());
    }
    match operator {
        BinaryOp::Concat => Scalar::Text(format!("{}{}", left.text(), right.text())),
        BinaryOp::Equal
        | BinaryOp::NotEqual
        | BinaryOp::Less
        | BinaryOp::LessEqual
        | BinaryOp::Greater
        | BinaryOp::GreaterEqual => {
            let ordering = compare(&left, &right);
            Scalar::Bool(match operator {
                BinaryOp::Equal => ordering == std::cmp::Ordering::Equal,
                BinaryOp::NotEqual => ordering != std::cmp::Ordering::Equal,
                BinaryOp::Less => ordering == std::cmp::Ordering::Less,
                BinaryOp::LessEqual => ordering != std::cmp::Ordering::Greater,
                BinaryOp::Greater => ordering == std::cmp::Ordering::Greater,
                BinaryOp::GreaterEqual => ordering != std::cmp::Ordering::Less,
                _ => unreachable!(),
            })
        }
        _ => {
            let left = match left.number() {
                Ok(value) => value,
                Err(error) => return error,
            };
            let right = match right.number() {
                Ok(value) => value,
                Err(error) => return error,
            };
            let value = match operator {
                BinaryOp::Add => left + right,
                BinaryOp::Subtract => left - right,
                BinaryOp::Multiply => left * right,
                BinaryOp::Divide if right == 0.0 => return div_zero_error(),
                BinaryOp::Divide => left / right,
                BinaryOp::Power => left.powf(right),
                _ => unreachable!(),
            };
            if value.is_finite() {
                Scalar::Number(value)
            } else {
                num_error()
            }
        }
    }
}

fn compare(left: &Scalar, right: &Scalar) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (left, right) {
        (Scalar::Blank, Scalar::Number(right)) => 0.0_f64.total_cmp(right),
        (Scalar::Number(left), Scalar::Blank) => left.total_cmp(&0.0),
        (Scalar::Blank, Scalar::Text(right)) => "".cmp(&right.to_ascii_lowercase()),
        (Scalar::Text(left), Scalar::Blank) => left.to_ascii_lowercase().as_str().cmp(""),
        (Scalar::Blank, Scalar::Bool(right)) => false.cmp(right),
        (Scalar::Bool(left), Scalar::Blank) => left.cmp(&false),
        (Scalar::Blank, Scalar::Blank) => Ordering::Equal,
        (Scalar::Number(left), Scalar::Number(right)) => left.total_cmp(right),
        (Scalar::Text(left), Scalar::Text(right)) => {
            left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase())
        }
        (Scalar::Bool(left), Scalar::Bool(right)) => left.cmp(right),
        (left, right) => type_rank(left).cmp(&type_rank(right)),
    }
}

fn type_rank(value: &Scalar) -> u8 {
    match value {
        Scalar::Blank | Scalar::Number(_) => 0,
        Scalar::Text(_) => 1,
        Scalar::Bool(_) => 2,
        Scalar::Error(_) => 3,
    }
}

fn round_excel(value: f64) -> f64 {
    // XLSX stores binary IEEE-754 values, while Excel's ROUND uses decimal
    // half-away-from-zero semantics. Correct the tiny representation error at
    // a decimal half boundary before applying that rule (for example,
    // 0.049999999999999975 rounded to one decimal place is 0.1 in Excel).
    let correction = f64::EPSILON * value.abs().max(1.0) * 4.0;
    let value = if value.is_sign_negative() {
        value - correction
    } else {
        value + correction
    };
    if value.is_sign_negative() {
        (value - 0.5).ceil()
    } else {
        (value + 0.5).floor()
    }
}

fn bound_changed_set(changed: &mut Vec<EvaluatedCell>, warnings: &mut Vec<String>) -> bool {
    if changed.len() <= RECALC_CHANGED_CAP {
        return false;
    }
    let omitted = changed.len() - RECALC_CHANGED_CAP;
    changed.truncate(RECALC_CHANGED_CAP);
    warnings.push(format!(
        "recalc changed-set truncated at {RECALC_CHANGED_CAP} cells; {omitted} omitted"
    ));
    true
}

/// Parser-only entry point for the cargo-fuzz target. It deliberately
/// discards the AST; success and every refusal are valid outcomes.
pub fn fuzz_parse_formula(input: &str) {
    let sheets = HashMap::from([("sheet1".to_owned(), 0)]);
    let _ = parse_formula(input, 0, &sheets);
}

/// Return whether a normalized formula is inside the exact v0.4 surface.
/// The harness uses this to define the denominator of its coverage row from
/// the same parser the shipped server uses.
pub fn is_supported_formula(formula: &str, current_sheet: u32, sheet_names: &[String]) -> bool {
    let sheets = sheet_names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| Some((name.to_ascii_lowercase(), u32::try_from(index).ok()?)))
        .collect::<HashMap<_, _>>();
    parse_formula(formula, current_sheet, &sheets).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wax_core::{Cell, Document, Sheet};

    fn cell(r: u32, c: u32, value: Option<CellValue>, formula: Option<&str>) -> Cell {
        let t = match &value {
            Some(CellValue::Number(_)) => CellType::N,
            Some(CellValue::Text(_)) => CellType::S,
            Some(CellValue::Bool(_)) => CellType::B,
            None => CellType::S,
        };
        Cell {
            r,
            c,
            t,
            v: value,
            d: None,
            f: formula.map(str::to_owned),
            fmt: None,
            s: None,
        }
    }

    fn workbook(cells: Vec<Cell>) -> WorkbookStore {
        WorkbookStore::from_document(Document::success(
            "test",
            "test.xlsx",
            vec![Sheet {
                name: "Sheet1".to_owned(),
                index: 0,
                rows: 32,
                cols: 32,
                truncated: false,
                merges: Vec::new(),
                cells,
                frozen_rows: 0,
                frozen_cols: 0,
                col_infos: Vec::new(),
                row_infos: Vec::new(),
                default_row_height: None,
                default_col_width: None,
            }],
            Vec::new(),
        ))
    }

    fn evaluate(formula: &str, precedents: Vec<Cell>) -> EvaluatedCell {
        let mut cells = precedents;
        cells.push(cell(10, 10, Some(CellValue::Number(-999.0)), Some(formula)));
        let store = workbook(cells);
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        assert_eq!(stats.evaluated, 1, "{formula}: {stats:?}");
        engine
            .evaluated_cell(CellRef {
                sheet: 0,
                r: 10,
                c: 10,
            })
            .cloned()
            .expect("formula should evaluate")
    }

    fn number(formula: &str, precedents: Vec<Cell>) -> f64 {
        let cell = evaluate(formula, precedents);
        let Some(CellValue::Number(value)) = cell.v else {
            panic!("{formula} did not produce a number: {cell:?}");
        };
        value
    }

    #[test]
    fn arithmetic_comparisons_concat_and_references() {
        assert_eq!(number("1+2*3^2-4/2", Vec::new()), 17.0);
        assert_eq!(number("50%", Vec::new()), 0.5);
        assert_eq!(evaluate("1<2", Vec::new()).v, Some(CellValue::Bool(true)));
        assert_eq!(
            evaluate(r#""a"&1&TRUE"#, Vec::new()).v,
            Some(CellValue::Text("a1TRUE".to_owned()))
        );
        assert_eq!(
            number(
                "$A$1+A$2+$A3+Sheet1!A4",
                vec![
                    cell(0, 0, Some(CellValue::Number(1.0)), None),
                    cell(1, 0, Some(CellValue::Number(2.0)), None),
                    cell(2, 0, Some(CellValue::Number(3.0)), None),
                    cell(3, 0, Some(CellValue::Number(4.0)), None),
                ],
            ),
            10.0
        );
    }

    #[test]
    fn cross_sheet_ranges_use_the_qualified_sheet_for_both_endpoints() {
        let formula_sheet = Sheet {
            name: "Summary".to_owned(),
            index: 0,
            rows: 1,
            cols: 1,
            truncated: false,
            merges: Vec::new(),
            cells: vec![cell(
                0,
                0,
                Some(CellValue::Number(0.0)),
                Some("SUM(Data!A1:B2)"),
            )],
            frozen_rows: 0,
            frozen_cols: 0,
            col_infos: Vec::new(),
            row_infos: Vec::new(),
            default_row_height: None,
            default_col_width: None,
        };
        let data_sheet = Sheet {
            name: "Data".to_owned(),
            index: 1,
            rows: 2,
            cols: 3,
            truncated: false,
            merges: Vec::new(),
            cells: vec![
                cell(0, 0, Some(CellValue::Number(1.0)), None),
                cell(0, 1, Some(CellValue::Number(2.0)), None),
                cell(1, 0, Some(CellValue::Number(3.0)), None),
                cell(1, 1, Some(CellValue::Number(4.0)), None),
                cell(
                    0,
                    2,
                    Some(CellValue::Number(99.0)),
                    Some("VLOOKUP(1,A1:B2,2,FALSE)"),
                ),
            ],
            frozen_rows: 0,
            frozen_cols: 0,
            col_infos: Vec::new(),
            row_infos: Vec::new(),
            default_row_height: None,
            default_col_width: None,
        };
        let store = WorkbookStore::from_document(Document::success(
            "test",
            "test.xlsx",
            vec![formula_sheet, data_sheet],
            Vec::new(),
        ));
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        assert_eq!(stats.evaluated, 1);
        assert_eq!(stats.skipped, 1);
        assert_eq!(engine.unevaluated_formulas(), 1);
        assert_eq!(engine.unevaluated_formulas_on_sheet(0), 0);
        assert_eq!(engine.unevaluated_formulas_on_sheet(1), 1);
        assert_eq!(
            engine
                .evaluated_cell(CellRef {
                    sheet: 0,
                    r: 0,
                    c: 0,
                })
                .and_then(|cell| cell.v.clone()),
            Some(CellValue::Number(10.0))
        );
    }

    #[test]
    fn exact_curated_function_set_and_coercion_edges() {
        let precedents = vec![
            cell(0, 0, Some(CellValue::Number(2.0)), None),
            cell(1, 0, Some(CellValue::Text("3".to_owned())), None),
            cell(2, 0, Some(CellValue::Bool(true)), None),
            cell(3, 0, None, None),
            cell(4, 0, Some(CellValue::Text("x".to_owned())), None),
        ];
        for (formula, expected) in [
            ("SUM(A1:A5,TRUE,\"4\")", 7.0),
            ("AVERAGE(A1:A5,4)", 3.0),
            ("COUNT(A1:A5,TRUE,\"4\")", 3.0),
            ("COUNTA(A1:A5)", 4.0),
            ("MIN(A1:A5,4)", 2.0),
            ("MAX(A1:A5,4)", 4.0),
            ("ROUND(2.675,2)", 2.68),
            ("ROUND(0.049999999999999975,1)", 0.1),
            ("ABS(-7)", 7.0),
        ] {
            assert!(
                (number(formula, precedents.clone()) - expected).abs() < 1e-12,
                "{formula}"
            );
        }
        assert_eq!(
            evaluate("IF(TRUE,\"yes\",1/0)", precedents.clone()).v,
            Some(CellValue::Text("yes".to_owned()))
        );
        assert_eq!(
            evaluate("AND(TRUE,1,A1)", precedents.clone()).v,
            Some(CellValue::Bool(true))
        );
        assert_eq!(
            evaluate("OR(FALSE,0,A3)", precedents.clone()).v,
            Some(CellValue::Bool(true))
        );
        assert_eq!(
            evaluate("NOT(FALSE)", precedents.clone()).v,
            Some(CellValue::Bool(true))
        );
        assert_eq!(
            evaluate("CONCAT(A1:A3,\"!\")", precedents).v,
            Some(CellValue::Text("23TRUE!".to_owned()))
        );
    }

    #[test]
    fn scalar_arithmetic_matches_excel_blank_text_and_boolean_coercion() {
        assert_eq!(number("A1+1", Vec::new()), 1.0, "missing cells are zero");
        assert_eq!(
            evaluate("A1", Vec::new()).v,
            Some(CellValue::Number(0.0)),
            "a scalar reference to a missing cell evaluates to zero"
        );
        assert_eq!(number("TRUE+1", Vec::new()), 2.0);
        assert_eq!(number("FALSE+1", Vec::new()), 1.0);
        assert_eq!(number(r#""2.5"+1"#, Vec::new()), 3.5);
        let not_numeric = evaluate(r#""wax"+1"#, Vec::new());
        assert_eq!(not_numeric.t, CellType::E);
        assert_eq!(not_numeric.v, Some(CellValue::Text("#VALUE!".to_owned())));
        assert_eq!(
            evaluate(
                "+A1",
                vec![cell(0, 0, Some(CellValue::Text(String::new())), None)]
            )
            .v,
            Some(CellValue::Text(String::new()))
        );
    }

    #[test]
    fn date_typed_formula_results_keep_iso_values_and_date_arithmetic() {
        let mut date = cell(0, 0, Some(CellValue::Text("1900-01-01".to_owned())), None);
        date.t = CellType::D;
        date.fmt = Some("yyyy-mm-dd".to_owned());
        let mut formula = cell(
            0,
            1,
            Some(CellValue::Text("1900-01-02".to_owned())),
            Some("A1+1"),
        );
        formula.t = CellType::D;
        formula.fmt = Some("yyyy-mm-dd".to_owned());
        let mut blank_formula = cell(
            0,
            2,
            Some(CellValue::Text("1899-12-30".to_owned())),
            Some("A2"),
        );
        blank_formula.t = CellType::D;
        blank_formula.fmt = Some("yyyy-mm-dd".to_owned());
        let mut leap_bug_date = cell(2, 0, Some(CellValue::Text("1900-02-29".to_owned())), None);
        leap_bug_date.t = CellType::D;
        let mut leap_bug_formula = cell(
            2,
            1,
            Some(CellValue::Text("1900-03-01".to_owned())),
            Some("A3+1"),
        );
        leap_bug_formula.t = CellType::D;
        leap_bug_formula.fmt = Some("yyyy-mm-dd".to_owned());
        let store = workbook(vec![
            date,
            formula,
            blank_formula,
            leap_bug_date,
            leap_bug_formula,
        ]);
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        assert_eq!(stats.evaluated, 3);
        let evaluated = engine
            .evaluated_cell(CellRef {
                sheet: 0,
                r: 0,
                c: 1,
            })
            .expect("date formula");
        assert_eq!(evaluated.t, CellType::D);
        assert_eq!(evaluated.v, Some(CellValue::Text("1900-01-02".to_owned())));
        assert_eq!(evaluated.d.as_deref(), Some("1900-01-02"));
        let blank_date = engine
            .evaluated_cell(CellRef {
                sheet: 0,
                r: 0,
                c: 2,
            })
            .expect("blank date formula");
        assert_eq!(blank_date.t, CellType::D);
        assert_eq!(blank_date.v, Some(CellValue::Text("1899-12-31".to_owned())));
        assert_eq!(
            engine
                .evaluated_cell(CellRef {
                    sheet: 0,
                    r: 2,
                    c: 1,
                })
                .and_then(|cell| cell.v.clone()),
            Some(CellValue::Text("1900-03-01".to_owned()))
        );
    }

    #[test]
    fn date_arithmetic_respects_the_workbook_1904_epoch() {
        let mut date = cell(0, 0, Some(CellValue::Text("1904-01-01".to_owned())), None);
        date.t = CellType::D;
        let mut formula = cell(
            0,
            1,
            Some(CellValue::Text("1904-01-02".to_owned())),
            Some("A1+1"),
        );
        formula.t = CellType::D;
        formula.fmt = Some("yyyy-mm-dd".to_owned());
        let mut document = Document::success(
            "test",
            "test.xlsx",
            vec![Sheet {
                name: "Sheet1".to_owned(),
                index: 0,
                rows: 1,
                cols: 2,
                truncated: false,
                merges: Vec::new(),
                cells: vec![date, formula],
                frozen_rows: 0,
                frozen_cols: 0,
                col_infos: Vec::new(),
                row_infos: Vec::new(),
                default_row_height: None,
                default_col_width: None,
            }],
            Vec::new(),
        );
        document.date_1904 = true;
        let store = WorkbookStore::from_document(document);
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        assert_eq!(stats.evaluated, 1);
        assert_eq!(
            engine
                .evaluated_cell(CellRef {
                    sheet: 0,
                    r: 0,
                    c: 1,
                })
                .and_then(|cell| cell.v.clone()),
            Some(CellValue::Text("1904-01-02".to_owned()))
        );
    }

    #[test]
    fn unknown_functions_stay_file_cached() {
        let store = workbook(vec![cell(
            0,
            0,
            Some(CellValue::Number(41.0)),
            Some("VLOOKUP(1,A1:B2,2,FALSE)"),
        )]);
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        assert!(engine.base_evaluated().is_empty());
        assert_eq!(stats.evaluated, 0);
        assert_eq!(stats.skipped, 1);
    }

    #[test]
    fn disabled_container_mode_never_marks_formula_caches_evaluated() {
        let store = workbook(vec![cell(
            0,
            0,
            Some(CellValue::Number(41.0)),
            Some("40+2"),
        )]);
        let (engine, stats) = FormulaWorkbook::file_cached(&store);
        assert_eq!(stats.evaluated, 0);
        assert_eq!(stats.skipped, 1);
        assert!(engine.base_evaluated().is_empty());
        assert_eq!(engine.unevaluated_formulas(), 1);
    }

    #[test]
    fn cycles_are_loud_and_do_not_hang() {
        let store = workbook(vec![
            cell(0, 0, Some(CellValue::Number(1.0)), Some("B1+1")),
            cell(0, 1, Some(CellValue::Number(2.0)), Some("A1+1")),
            cell(0, 2, Some(CellValue::Number(3.0)), Some("A1+1")),
        ]);
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        assert_eq!(stats.evaluated, 3);
        for c in 0..3 {
            assert_eq!(
                engine
                    .evaluated_cell(CellRef { sheet: 0, r: 0, c })
                    .and_then(|cell| cell.v.clone()),
                Some(CellValue::Text("#CYCLE!".to_owned()))
            );
        }
    }

    #[test]
    fn overriding_a_formula_can_break_a_cycle_for_that_recalc_only() {
        let store = workbook(vec![
            cell(0, 0, Some(CellValue::Number(1.0)), Some("B1+1")),
            cell(0, 1, Some(CellValue::Number(2.0)), Some("A1+1")),
        ]);
        let (engine, _) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        let outcome = engine
            .recalculate(
                &store,
                &[CellOverride {
                    sheet: 0,
                    r: 0,
                    c: 0,
                    v: Some(CellValue::Number(10.0)),
                    f: None,
                }],
                Duration::from_secs(1),
            )
            .expect("an override should break the cycle");
        assert_eq!(outcome.evaluated, 1);
        assert_eq!(outcome.changed.len(), 2);
        assert_eq!(outcome.changed[0].c, 0);
        assert_eq!(outcome.changed[0].v, Some(CellValue::Number(10.0)));
        assert!(!outcome.changed[0].e);
        assert_eq!(outcome.changed[1].c, 1);
        assert_eq!(outcome.changed[1].v, Some(CellValue::Number(11.0)));
        assert_eq!(
            engine
                .evaluated_cell(CellRef {
                    sheet: 0,
                    r: 0,
                    c: 1,
                })
                .and_then(|cell| cell.v.clone()),
            Some(CellValue::Text("#CYCLE!".to_owned())),
            "the base overlay must remain immutable"
        );
    }

    #[test]
    fn zero_budget_is_loud_and_keeps_the_file_cache() {
        let store = workbook(vec![cell(
            0,
            0,
            Some(CellValue::Number(41.0)),
            Some("40+2"),
        )]);
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::ZERO);
        assert!(engine.base_evaluated().is_empty());
        assert_eq!(stats.evaluated, 0);
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.warnings.len(), 1);
        assert!(stats.warnings[0].contains("budget exceeded"));
        assert!(stats.warnings[0].contains("file-cached"));
    }

    #[test]
    fn cycle_marking_obeys_the_same_wall_clock_budget() {
        let store = workbook(vec![
            cell(0, 0, Some(CellValue::Number(1.0)), Some("B1+1")),
            cell(0, 1, Some(CellValue::Number(2.0)), Some("A1+1")),
        ]);
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::ZERO);
        assert!(engine.base_evaluated().is_empty());
        assert_eq!(stats.evaluated, 0);
        assert_eq!(stats.skipped, 2);
        assert_eq!(stats.warnings.len(), 1);
        assert!(stats.warnings[0].contains("budget exceeded"));
    }

    #[test]
    fn evaluated_display_uses_the_retained_format_code() {
        let mut formula = cell(0, 0, Some(CellValue::Number(0.0)), Some("1234.5"));
        formula.fmt = Some("#,##0.00".to_owned());
        let store = workbook(vec![formula]);
        let (engine, stats) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        assert_eq!(stats.evaluated, 1);
        let evaluated = engine
            .evaluated_cell(CellRef {
                sheet: 0,
                r: 0,
                c: 0,
            })
            .expect("formula should evaluate");
        assert_eq!(evaluated.d.as_deref(), Some("1,234.50"));
        assert!(evaluated.e);
    }

    #[test]
    fn recalc_is_last_wins_incremental_and_side_effect_free() {
        let mut formatted_formula = cell(0, 1, Some(CellValue::Number(4.0)), Some("A1*2"));
        formatted_formula.fmt = Some("0.00".to_owned());
        let store = workbook(vec![
            cell(0, 0, Some(CellValue::Number(2.0)), None),
            formatted_formula,
            cell(0, 2, Some(CellValue::Number(5.0)), Some("B1+1")),
            cell(0, 3, Some(CellValue::Number(99.0)), Some("SUM(A5:A6)")),
        ]);
        let (engine, _) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        let outcome = engine
            .recalculate(
                &store,
                &[
                    CellOverride {
                        sheet: 0,
                        r: 0,
                        c: 0,
                        v: Some(CellValue::Number(3.0)),
                        f: None,
                    },
                    CellOverride {
                        sheet: 0,
                        r: 0,
                        c: 0,
                        v: Some(CellValue::Number(4.0)),
                        f: None,
                    },
                ],
                Duration::from_secs(1),
            )
            .expect("recalc should succeed");
        assert_eq!(outcome.evaluated, 2);
        assert_eq!(outcome.changed.len(), 3);
        assert_eq!(outcome.changed[0].v, Some(CellValue::Number(4.0)));
        assert!(!outcome.changed[0].e);
        assert_eq!(outcome.changed[1].v, Some(CellValue::Number(8.0)));
        assert_eq!(outcome.changed[1].d.as_deref(), Some("8.00"));
        assert!(outcome.changed[1].e);
        assert_eq!(outcome.changed[2].v, Some(CellValue::Number(9.0)));
        assert_eq!(
            engine
                .evaluated_cell(CellRef {
                    sheet: 0,
                    r: 0,
                    c: 1,
                })
                .and_then(|cell| cell.v.clone()),
            Some(CellValue::Number(4.0))
        );
    }

    #[test]
    fn authored_formulas_join_the_graph_render_general_and_report_errors() {
        let mut formatted = cell(0, 0, Some(CellValue::Number(2.0)), None);
        formatted.fmt = Some("0.00".to_owned());
        let mut formatted_dependent = cell(0, 2, Some(CellValue::Number(0.0)), Some("B1*2"));
        formatted_dependent.fmt = Some("0.00".to_owned());
        let store = workbook(vec![formatted, formatted_dependent]);
        let (engine, _) = FormulaWorkbook::open(&store, Duration::from_secs(1));
        let outcome = engine
            .recalculate(
                &store,
                &[
                    CellOverride {
                        sheet: 0,
                        r: 0,
                        c: 0,
                        v: Some(CellValue::Number(4.0)),
                        f: None,
                    },
                    CellOverride {
                        sheet: 0,
                        r: 0,
                        c: 1,
                        v: Some(CellValue::Number(999.0)),
                        f: Some("=A1+1".to_owned()),
                    },
                ],
                Duration::from_secs(1),
            )
            .expect("authored formula recalc should succeed");
        assert_eq!(outcome.evaluated, 2);
        assert_eq!(outcome.changed.len(), 3);
        assert_eq!(outcome.changed[0].d.as_deref(), Some("4.00"));
        assert!(!outcome.changed[0].e);
        assert_eq!(outcome.changed[1].v, Some(CellValue::Number(5.0)));
        assert_eq!(outcome.changed[1].d.as_deref(), Some("5"));
        assert!(outcome.changed[1].e);
        assert_eq!(outcome.changed[2].v, Some(CellValue::Number(10.0)));
        assert_eq!(outcome.changed[2].d.as_deref(), Some("10.00"));

        let errors = engine
            .recalculate(
                &store,
                &[
                    CellOverride {
                        sheet: 0,
                        r: 0,
                        c: 3,
                        v: None,
                        f: Some("=NOPE(A1)".to_owned()),
                    },
                    CellOverride {
                        sheet: 0,
                        r: 0,
                        c: 4,
                        v: None,
                        f: Some("=1+".to_owned()),
                    },
                    CellOverride {
                        sheet: 0,
                        r: 0,
                        c: 5,
                        v: None,
                        f: Some("=G1+1".to_owned()),
                    },
                    CellOverride {
                        sheet: 0,
                        r: 0,
                        c: 6,
                        v: None,
                        f: Some("=F1+1".to_owned()),
                    },
                ],
                Duration::from_secs(1),
            )
            .expect("engine errors belong to cells, not the request");
        let values = errors
            .changed
            .iter()
            .filter(|cell| cell.c >= 3)
            .map(|cell| (cell.c, cell.v.clone(), cell.d.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![
                (
                    3,
                    Some(CellValue::Text("#NAME?".to_owned())),
                    Some("#NAME?".to_owned())
                ),
                (
                    4,
                    Some(CellValue::Text("#VALUE!".to_owned())),
                    Some("#VALUE!".to_owned())
                ),
                (
                    5,
                    Some(CellValue::Text("#CYCLE!".to_owned())),
                    Some("#CYCLE!".to_owned())
                ),
                (
                    6,
                    Some(CellValue::Text("#CYCLE!".to_owned())),
                    Some("#CYCLE!".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn recalc_changed_set_is_bounded_and_loud() {
        let mut changed = (0..=RECALC_CHANGED_CAP)
            .map(|index| EvaluatedCell {
                sheet: 0,
                r: u32::try_from(index).expect("test index"),
                c: 0,
                t: CellType::N,
                v: Some(CellValue::Number(index as f64)),
                d: None,
                e: true,
            })
            .collect::<Vec<_>>();
        let mut warnings = Vec::new();
        assert!(bound_changed_set(&mut changed, &mut warnings));
        assert_eq!(changed.len(), RECALC_CHANGED_CAP);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("50000"));
        assert!(warnings[0].contains("1 omitted"));
    }

    #[test]
    fn parser_handles_quoted_sheets_and_rejects_malformed_input() {
        let sheets = HashMap::from([("a b".to_owned(), 0)]);
        assert!(parse_formula("'A B'!$A$1", 0, &sheets).is_ok());
        for formula in [
            "",
            "SUM(",
            "NOPE(1)",
            "A0",
            "A1:B",
            "#REF!",
            "A1:A2",
            "A1:A2*B1:B2",
            "ABS(A1:A2)",
        ] {
            assert!(parse_formula(formula, 0, &sheets).is_err(), "{formula}");
        }
        assert!(parse_formula("SUM(A1:A2)", 0, &sheets).is_ok());

        let deeply_nested = format!("{}1{}", "(".repeat(300), ")".repeat(300));
        assert!(matches!(
            parse_formula(&deeply_nested, 0, &sheets),
            Err(ParseError::TooLarge)
        ));
        let deep_unary = format!("{}1", "-".repeat(300));
        assert!(matches!(
            parse_formula(&deep_unary, 0, &sheets),
            Err(ParseError::TooLarge)
        ));
    }
}
