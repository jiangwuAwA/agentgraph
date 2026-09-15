use anyhow::Result;

use crate::index::store::Store;
use crate::model::{ImpactNode, ReferenceRecord, SymbolRecord};

pub struct Query<'a> {
    store: &'a Store,
}

impl<'a> Query<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub fn find_symbol(&self, name: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        self.store.find_symbol(name, limit)
    }

    pub fn callers(&self, name: &str, limit: usize) -> Result<Vec<ReferenceRecord>> {
        self.store.callers(name, limit)
    }

    pub fn impact(&self, name: &str, depth: usize, limit: usize) -> Result<Vec<ImpactNode>> {
        self.store.impact(name, depth, limit)
    }

    pub fn related_files(&self, name: &str, limit: usize) -> Result<Vec<(String, usize, String)>> {
        self.store.related_files(name, limit)
    }
}
