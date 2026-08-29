// The grammar generates the complete typed surface before every accessor has a
// semantic consumer. Keeping the surface complete makes codegen drift visible.
#![allow(dead_code)]

mod generated;

use super::{SyntaxKind, SyntaxNode};

pub(crate) use generated::{
    Array, ArrayTable, AstNode, InlineTable, Key, KeyValue, Root, Statement, Table, Value,
};

impl Root {
    pub(crate) fn statements(&self) -> impl Iterator<Item = Statement> + '_ {
        self.syntax().children().filter_map(Statement::cast)
    }
}

impl KeyValue {
    pub(crate) fn key(&self) -> Option<Key> {
        child(self.syntax())
    }

    pub(crate) fn value(&self) -> Option<Value> {
        child(self.syntax())
    }
}

impl Table {
    pub(crate) fn key(&self) -> Option<Key> {
        child(self.syntax())
    }
}

impl ArrayTable {
    pub(crate) fn key(&self) -> Option<Key> {
        child(self.syntax())
    }
}

impl Value {
    pub(crate) fn array(&self) -> Option<Array> {
        child(self.syntax())
    }

    pub(crate) fn inline_table(&self) -> Option<InlineTable> {
        child(self.syntax())
    }
}

impl Array {
    pub(crate) fn values(&self) -> impl Iterator<Item = Value> + '_ {
        children(self.syntax())
    }
}

impl InlineTable {
    pub(crate) fn entries(&self) -> impl Iterator<Item = KeyValue> + '_ {
        children(self.syntax())
    }
}

fn child<N: AstNode>(node: &SyntaxNode) -> Option<N> {
    node.children().find_map(N::cast)
}

fn children<'node, N: AstNode + 'node>(node: &'node SyntaxNode) -> impl Iterator<Item = N> + 'node {
    node.children().filter_map(N::cast)
}
