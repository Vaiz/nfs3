use intaglio::Symbol;

// TODO: use faster hash
// TODO: compress vec of symbols
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SymbolsPath(Vec<Symbol>);

impl SymbolsPath {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn join(&self, symbol: Symbol) -> Self {
        let mut new_path = self.clone();
        new_path.0.push(symbol);
        new_path
    }

    pub fn symbols(&self) -> impl Iterator<Item = Symbol> {
        self.0.iter().copied()
    }
}
