mod basic;
mod counter;

use anyhow::Result;
use std::path::Path;

pub struct TemplateGenerator {
    name: String,
    org: String,
}

impl TemplateGenerator {
    pub fn new(name: String, org: String) -> Self {
        Self { name, org }
    }

    pub fn generate_counter(&self, dir: &Path) -> Result<()> {
        counter::generate(dir, &self.name, &self.org)
    }

    pub fn generate_basic(&self, dir: &Path) -> Result<()> {
        basic::generate(dir, &self.name, &self.org)
    }

    pub fn generate_todo(&self, dir: &Path) -> Result<()> {
        // TODO: Implement todo template
        self.generate_basic(dir)
    }

    pub fn generate_dashboard(&self, dir: &Path) -> Result<()> {
        // TODO: Implement dashboard template
        self.generate_basic(dir)
    }

    pub fn generate_widget(&self, dir: &Path) -> Result<()> {
        // TODO: Implement widget package template
        self.generate_basic(dir)
    }

    pub fn generate_plugin(&self, dir: &Path) -> Result<()> {
        // TODO: Implement plugin template
        self.generate_basic(dir)
    }

    pub fn generate_empty(&self, dir: &Path) -> Result<()> {
        // TODO: Implement empty template
        self.generate_basic(dir)
    }
}
