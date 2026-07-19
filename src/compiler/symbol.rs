//! Atomix 符号表 — 分层栈式作用域管理。
//!
//! 完整覆盖 编译管线.md §4.1 的符号表规范。
//!
//! 层级:
//!   Level 0: 文件级（USE 导入、EXCEPTION、enum、type 别名、INPUT 常量、WORKS 模板名）
//!   Level 1: 区域级（TOOLS 函数名、WORKS 属性/方法、TASK 局部变量）
//!   Level 2+: 块级（IF/FOR 体、TRY 块、匿名函数体）

use crate::compiler::ast::{FuncDef, TypeNode};
use std::collections::HashMap;

// ─── 符号种类 ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// 变量
    Variable,
    /// 函数
    Function,
    /// 内置函数（编译期展开为 IR）
    Builtin,
    /// 常量
    Const,
    /// 类型别名 / enum 名
    Type,
    /// WORKS 模板
    Works,
    /// 枚举变体
    EnumVariant,
    /// 异常类型
    Exception,
}

// ─── 语义类型 ──────────────────────────────────────────

/// 解析后的语义类型（与语法层的 TypeNode 分离）。
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Bytes,
    Duration,
    List(Box<Type>),
    Dict(Box<Type>, Box<Type>),
    Tuple(Vec<Type>),
    /// 具名类型（枚举名 / 类型别名）
    Named(String),
    /// 推导失败标记（非运行时类型）
    Any,
    /// void（函数无返回值）
    Void,
    /// 类型错误
    Error,
}

/// 将语法类型节点解析为语义类型。
pub fn resolve_type(node: &TypeNode) -> Type {
    match node {
        TypeNode::Base(name) => match name.as_str() {
            "int" => Type::Int,
            "float" => Type::Float,
            "bool" => Type::Bool,
            "str" => Type::Str,
            "bytes" => Type::Bytes,
            "duration" => Type::Duration,
            _ => Type::Named(name.clone()),
        },
        TypeNode::List(inner) => Type::List(Box::new(resolve_type(inner))),
        TypeNode::Dict(k, v) => Type::Dict(Box::new(resolve_type(k)), Box::new(resolve_type(v))),
        TypeNode::Tuple(types) => Type::Tuple(types.iter().map(resolve_type).collect()),
        TypeNode::Named(name) => Type::Named(name.clone()),
        TypeNode::GenericParam(name) => Type::Named(name.clone()),
    }
}

// ─── 符号条目 ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub resolved_type: Option<Type>,
    /// 是否是 GOOUT 产出变量
    pub is_goout: bool,
    /// 是否是 PUB（公开）
    pub is_public: bool,
    /// 函数定义（仅 Function 类型时有）
    pub func_def: Option<Box<FuncDef>>,
}

impl Symbol {
    pub fn new(name: String, kind: SymbolKind) -> Self {
        Self {
            name,
            kind,
            resolved_type: None,
            is_goout: false,
            is_public: false,
            func_def: None,
        }
    }

    pub fn with_type(mut self, t: Type) -> Self {
        self.resolved_type = Some(t);
        self
    }

    pub fn with_public(mut self, pub_: bool) -> Self {
        self.is_public = pub_;
        self
    }

    pub fn with_func(mut self, f: FuncDef) -> Self {
        self.func_def = Some(Box::new(f));
        self
    }
}

// ─── 符号表 ────────────────────────────────────────────

/// 分层栈式符号表。
#[derive(Debug, Clone)]
pub struct SymbolTable {
    /// 作用域栈。index 0 = 全局 (Level 0), 后续为嵌套作用域。
    scopes: Vec<HashMap<String, Symbol>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()], // Level 0: 全局
        }
    }

    /// 推入新作用域（Level 1 或 Level 2+）。
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// 弹出作用域。
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// 声明符号。同一作用域内重名报错。
    pub fn declare(&mut self, symbol: Symbol) -> Result<(), String> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(&symbol.name) {
            return Err(format!("重复声明 `{}`", symbol.name));
        }
        scope.insert(symbol.name.clone(), symbol);
        Ok(())
    }

    /// 查找符号（从当前作用域向上查找）。
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(s) = scope.get(name) {
                return Some(s);
            }
        }
        None
    }

    /// 查找符号的可变引用。
    pub fn lookup_mut(&mut self, name: &str) -> Option<&mut Symbol> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                return scope.get_mut(name);
            }
        }
        None
    }

    /// 检查当前作用域是否包含该名称（不向上查找）。
    pub fn contains_current(&self, name: &str) -> bool {
        self.scopes.last().unwrap().contains_key(name)
    }

    /// 检查名称是否已声明（向上查找）。
    pub fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    /// 获取当前作用域中所有符号的迭代器（用于遍历局部符号）。
    pub fn current_scope(&self) -> impl Iterator<Item = (&String, &Symbol)> {
        self.scopes.last().unwrap().iter()
    }

    /// 获取所有函数定义（用于 CALL 解析）。
    pub fn functions(&self) -> Vec<&Symbol> {
        let mut fns = Vec::new();
        for scope in &self.scopes {
            for sym in scope.values() {
                if sym.kind == SymbolKind::Function {
                    fns.push(sym);
                }
            }
        }
        fns
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_declare_and_lookup() {
        let mut st = SymbolTable::new();
        st.declare(Symbol::new("x".into(), SymbolKind::Variable))
            .unwrap();
        assert!(st.contains("x"));
        assert_eq!(st.lookup("x").unwrap().kind, SymbolKind::Variable);
    }

    #[test]
    fn duplicate_declare_error() {
        let mut st = SymbolTable::new();
        st.declare(Symbol::new("x".into(), SymbolKind::Variable))
            .unwrap();
        assert!(
            st.declare(Symbol::new("x".into(), SymbolKind::Variable))
                .is_err()
        );
    }

    #[test]
    fn scoped_shadowing() {
        let mut st = SymbolTable::new();
        st.declare(Symbol::new("x".into(), SymbolKind::Variable))
            .unwrap();
        st.push_scope();
        // Shadowing allowed in inner scope
        st.declare(Symbol::new("x".into(), SymbolKind::Variable))
            .unwrap();
        assert_eq!(st.lookup("x").unwrap().kind, SymbolKind::Variable);
        st.pop_scope();
        assert_eq!(st.lookup("x").unwrap().kind, SymbolKind::Variable);
    }

    #[test]
    fn scope_lifetime() {
        let mut st = SymbolTable::new();
        st.declare(Symbol::new("outer".into(), SymbolKind::Variable))
            .unwrap();
        st.push_scope();
        st.declare(Symbol::new("inner".into(), SymbolKind::Variable))
            .unwrap();
        assert!(st.contains("inner"));
        st.pop_scope();
        assert!(!st.contains("inner"));
        assert!(st.contains("outer"));
    }

    #[test]
    fn resolve_type_node() {
        assert_eq!(resolve_type(&TypeNode::Base("int".into())), Type::Int);
        assert_eq!(resolve_type(&TypeNode::Base("float".into())), Type::Float);
        assert_eq!(
            resolve_type(&TypeNode::List(Box::new(TypeNode::Base("int".into())))),
            Type::List(Box::new(Type::Int))
        );
    }

    #[test]
    fn symbol_with_type_and_func() {
        let sym = Symbol::new("add".into(), SymbolKind::Function)
            .with_type(Type::Int)
            .with_public(true);
        assert_eq!(sym.resolved_type, Some(Type::Int));
        assert!(sym.is_public);
    }
}
