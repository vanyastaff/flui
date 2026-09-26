//! A `ScopeRef` can be handed to a signal read and nothing else: neither its
//! graph nor its sink can be pulled back out, so holding a scope is not a path
//! to the graph (ADR-0085 §2).
fn pull_out(scope: flui_view::ScopeRef<'_>) {
    let _graph = scope.graph;
    let _graph = scope.graph();
    let _sink = scope.sink;
}

fn main() {}
