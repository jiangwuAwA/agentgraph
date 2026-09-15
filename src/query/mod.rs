use anyhow::Result;

use crate::index::store::Store;
use crate::model::{ConfidenceFilter, ImpactNode, ReferenceRecord, SymbolRecord};

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
        self.store
            .callers_filtered(name, limit, ConfidenceFilter::Default)
    }

    pub fn callers_filtered(
        &self,
        name: &str,
        limit: usize,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ReferenceRecord>> {
        self.store.callers_filtered(name, limit, filter)
    }

    pub fn impact(&self, name: &str, depth: usize, limit: usize) -> Result<Vec<ImpactNode>> {
        self.store
            .impact_filtered(name, depth, limit, ConfidenceFilter::Default)
    }

    pub fn impact_filtered(
        &self,
        name: &str,
        depth: usize,
        limit: usize,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ImpactNode>> {
        self.store.impact_filtered(name, depth, limit, filter)
    }

    pub fn related_files(&self, name: &str, limit: usize) -> Result<Vec<(String, usize, String)>> {
        self.store.related_files(name, limit)
    }
}

/// CLI/MCP string flag → filter. Default = Exact + Heuristic.
pub fn parse_confidence_flags(exact_only: bool, include_dynamic: bool) -> ConfidenceFilter {
    if exact_only {
        ConfidenceFilter::ExactOnly
    } else if include_dynamic {
        ConfidenceFilter::IncludeDynamic
    } else {
        ConfidenceFilter::Default
    }
}
