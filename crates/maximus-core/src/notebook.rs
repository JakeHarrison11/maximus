//! The notebook document: cells that can be edited and re-ordered independently of the Maxima state.

use maximus_math::{from_latex, maxima_to_latex, row_to_latex, tree_to_maxima, ParseCtx};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellKind {
    Code,
    Text,
    Title,
    Section,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMode {
    /// Textbook math; stored as LaTeX.
    Math,
    /// Plain Maxima source.
    Maxima,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum OutputItem {
    Text(String),
    Result { label: String, text: String, tex: String },
    Error(String),
    Plot(crate::PlotData),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CellState {
    #[default]
    Idle,
    Queued,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cell {
    pub id: u64,
    pub kind: CellKind,
    pub mode: InputMode,
    /// LaTeX in `Math` mode, otherwise raw text.
    pub source: String,
    pub outputs: Vec<OutputItem>,
    #[serde(skip)]
    pub state: CellState,
    pub collapsed: bool,
}

impl Cell {
    pub fn new(id: u64, kind: CellKind, mode: InputMode) -> Self {
        Cell { id, kind, mode, source: String::new(), outputs: Vec::new(), state: CellState::Idle, collapsed: false }
    }

    /// Translates the cell input into Maxima statements.
    pub fn to_maxima(&self, ctx: &ParseCtx) -> Result<String, String> {
        match self.mode {
            InputMode::Maxima => Ok(ensure_terminated(&self.source)),
            InputMode::Math => tree_to_maxima(&from_latex(&self.source), ctx),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.source.trim().is_empty()
    }

    /// The cell input as LaTeX (converting Maxima source is not attempted).
    pub fn latex(&self) -> Option<String> {
        match self.mode {
            InputMode::Math => Some(row_to_latex(&from_latex(&self.source))),
            InputMode::Maxima => None,
        }
    }
}

pub fn ensure_terminated(src: &str) -> String {
    let t = src.trim_end();
    if t.is_empty() || t.ends_with(';') || t.ends_with('$') {
        t.to_string()
    } else {
        format!("{};", t)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Notebook {
    pub version: u32,
    pub cells: Vec<Cell>,
    next_id: u64,
}

impl Default for Notebook {
    fn default() -> Self {
        let mut nb = Notebook { version: 1, cells: Vec::new(), next_id: 1 };
        nb.add_cell(CellKind::Code, InputMode::Math);
        nb
    }
}

impl Notebook {
    pub fn empty() -> Self {
        Notebook { version: 1, cells: Vec::new(), next_id: 1 }
    }

    pub fn add_cell(&mut self, kind: CellKind, mode: InputMode) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.cells.push(Cell::new(id, kind, mode));
        id
    }

    pub fn insert_cell(&mut self, index: usize, kind: CellKind, mode: InputMode) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let index = index.min(self.cells.len());
        self.cells.insert(index, Cell::new(id, kind, mode));
        id
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.cells.iter().position(|c| c.id == id)
    }

    pub fn cell(&self, id: u64) -> Option<&Cell> {
        self.cells.iter().find(|c| c.id == id)
    }

    pub fn cell_mut(&mut self, id: u64) -> Option<&mut Cell> {
        self.cells.iter_mut().find(|c| c.id == id)
    }

    pub fn remove(&mut self, id: u64) -> Option<Cell> {
        let i = self.index_of(id)?;
        Some(self.cells.remove(i))
    }

    pub fn move_up(&mut self, id: u64) {
        if let Some(i) = self.index_of(id) {
            if i > 0 {
                self.cells.swap(i, i - 1);
            }
        }
    }

    pub fn move_down(&mut self, id: u64) {
        if let Some(i) = self.index_of(id) {
            if i + 1 < self.cells.len() {
                self.cells.swap(i, i + 1);
            }
        }
    }

    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let i = self.index_of(id)?;
        let mut c = self.cells[i].clone();
        c.id = self.next_id;
        self.next_id += 1;
        c.outputs.clear();
        c.state = CellState::Idle;
        let nid = c.id;
        self.cells.insert(i + 1, c);
        Some(nid)
    }

    pub fn clear_outputs(&mut self) {
        for c in &mut self.cells {
            c.outputs.clear();
            c.state = CellState::Idle;
        }
    }

    /// A runnable Maxima batch file with the code cells (text cells become comments).
    pub fn to_maxima_script(&self, ctx: &ParseCtx) -> String {
        let mut out = String::new();
        for c in &self.cells {
            match c.kind {
                CellKind::Code => match c.to_maxima(ctx) {
                    Ok(code) if !code.trim().is_empty() => {
                        out.push_str(&code);
                        out.push_str("\n\n");
                    }
                    Ok(_) => {}
                    Err(e) => out.push_str(&format!("/* input error: {} */\n\n", e.replace("*/", ""))),
                },
                _ => out.push_str(&format!("/* {} */\n\n", c.source.replace("*/", "* /"))),
            }
        }
        out
    }

    /// A standalone LaTeX document with inputs and results typeset as math.
    pub fn to_latex_document(&self) -> String {
        let esc = |s: &str| s.replace('\\', "\\textbackslash{}").replace('&', "\\&").replace('%', "\\%").replace('#', "\\#").replace('_', "\\_");
        let mut out = String::from("\\documentclass{article}\n\\usepackage{amsmath,amssymb}\n\\begin{document}\n");
        for c in &self.cells {
            match c.kind {
                CellKind::Title => out.push_str(&format!("\\section*{{{}}}\n", esc(&c.source))),
                CellKind::Section => out.push_str(&format!("\\subsection*{{{}}}\n", esc(&c.source))),
                CellKind::Text => out.push_str(&format!("{}\n\n", esc(&c.source))),
                CellKind::Code => {
                    match c.mode {
                        InputMode::Math => out.push_str(&format!("\\[ {} \\]\n", row_to_latex(&from_latex(&c.source)))),
                        InputMode::Maxima => out.push_str(&format!("\\begin{{verbatim}}\n{}\n\\end{{verbatim}}\n", c.source)),
                    }
                    for o in &c.outputs {
                        match o {
                            OutputItem::Result { text, tex, .. } => {
                                let l = maxima_to_latex(text).unwrap_or_else(|_| tex.trim_matches('$').to_string());
                                out.push_str(&format!("\\[ {} \\]\n", l));
                            }
                            OutputItem::Text(t) | OutputItem::Error(t) => out.push_str(&format!("\\begin{{verbatim}}\n{}\n\\end{{verbatim}}\n", t)),
                            OutputItem::Plot(_) => out.push_str("\\textit{(plot)}\n"),
                        }
                    }
                }
            }
        }
        out.push_str("\\end{document}\n");
        out
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Result<Notebook, String> {
        let mut nb: Notebook = serde_json::from_str(s).map_err(|e| e.to_string())?;
        let max = nb.cells.iter().map(|c| c.id).max().unwrap_or(0);
        nb.next_id = nb.next_id.max(max + 1);
        Ok(nb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notebook_roundtrip() {
        let mut nb = Notebook::default();
        let id = nb.cells[0].id;
        nb.cell_mut(id).unwrap().source = "\\frac{1}{2}".into();
        let json = nb.to_json();
        let back = Notebook::from_json(&json).unwrap();
        assert_eq!(back.cells[0].source, "\\frac{1}{2}");
        assert_eq!(back.cells[0].to_maxima(&ParseCtx::default()).unwrap(), "1/2;");
    }

    #[test]
    fn exports() {
        let mut nb = Notebook::default();
        let id = nb.cells[0].id;
        nb.cell_mut(id).unwrap().source = "\\frac{1}{2}+x".into();
        nb.cell_mut(id).unwrap().outputs.push(OutputItem::Result { label: "%o1".into(), text: "x+1/2".into(), tex: String::new() });
        assert_eq!(nb.to_maxima_script(&ParseCtx::default()).trim(), "1/2+x;");
        let tex = nb.to_latex_document();
        assert!(tex.contains("\\frac{1}{2}+x") && tex.contains("\\begin{document}"), "{tex}");
    }

    #[test]
    fn cells_reorder_independently() {
        let mut nb = Notebook::default();
        let a = nb.cells[0].id;
        let b = nb.add_cell(CellKind::Text, InputMode::Maxima);
        nb.move_up(b);
        assert_eq!(nb.cells[0].id, b);
        assert_eq!(nb.cells[1].id, a);
    }
}
