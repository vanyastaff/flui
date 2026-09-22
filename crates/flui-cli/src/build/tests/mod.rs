//! Artifact-selection acceptance tests for the build pipeline. They build
//! real Cargo fixtures and re-execute this test binary as a worker (see
//! each module's `worker`), so they live next to the code as unit modules of
//! the `flui` binary rather than as a separate library test target.

mod desktop_artifacts;
mod ios_artifacts;
