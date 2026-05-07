use crate::plan::logical::{CompiledExpr, JoinType};
use crate::types::Schema;

/// Physical plan - the executor will create physical operators from this.
#[derive(Debug)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        schema: Schema,
        filter: Option<CompiledExpr>,
    },
    IndexScan {
        table: String,
        schema: Schema,
        index_name: String,
        key: CompiledExpr,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: CompiledExpr,
        schema: Schema,
    },
    Project {
        input: Box<PhysicalPlan>,
        exprs: Vec<(CompiledExpr, Option<String>)>,
        schema: Schema,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        condition: CompiledExpr,
        join_type: JoinType,
        schema: Schema,
    },
    Sort {
        input: Box<PhysicalPlan>,
        keys: Vec<(CompiledExpr, bool)>,
        schema: Schema,
    },
    Limit {
        input: Box<PhysicalPlan>,
        count: usize,
        offset: usize,
        schema: Schema,
    },
    Insert {
        table: String,
        rows: Vec<Vec<CompiledExpr>>,
        columns: Vec<String>,
        schema: Schema,
    },
    Update {
        table: String,
        assignments: Vec<(String, CompiledExpr)>,
        filter: Option<CompiledExpr>,
        schema: Schema,
    },
    Delete {
        table: String,
        filter: Option<CompiledExpr>,
        schema: Schema,
    },
    Empty {
        schema: Schema,
    },
}
