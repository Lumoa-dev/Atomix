//! Atomix 类型检查引擎 — 自底向上类型合成。
//!
//! 覆盖 类型系统.md §4 和 编译管线.md §4.3 的类型检查规范。
//!
//! 核心算法：从表达式叶节点出发，按运算规则由子节点类型推导父节点类型。
//! 类型不兼容时报告错误并标记为 Any 继续（最大努力模式）。

use crate::compiler::ast::{BinOp, Expr, UnOp};
use crate::compiler::symbol::{Type, SymbolTable};

// ─── 类型错误 ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
}

impl TypeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

// ─── 类型检查器 ────────────────────────────────────────

pub struct TypeChecker {
    pub errors: Vec<TypeError>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// 推导表达式的类型（自底向上）。
    pub fn infer_expr(&mut self, expr: &Expr, st: &SymbolTable) -> Type {
        match expr {
            Expr::Int(_) => Type::Int,
            Expr::Float(_) => Type::Float,
            Expr::Str(_) | Expr::FStr(_) => Type::Str,
            Expr::Bool(_) => Type::Bool,

            Expr::Ident(name) => {
                if let Some(sym) = st.lookup(name) {
                    sym.resolved_type.clone().unwrap_or(Type::Any)
                } else {
                    self.errors.push(TypeError::new(format!("未定义的标识符 `{name}`")));
                    Type::Any
                }
            }

            Expr::Binary { op, lhs, rhs } => self.check_binary(*op, lhs, rhs, st),
            Expr::Unary { op, expr: inner } => self.check_unary(*op, inner, st),

            Expr::List(items) => {
                if items.is_empty() {
                    Type::List(Box::new(Type::Any))
                } else {
                    let elem_type = self.infer_expr(&items[0], st);
                    // 检查所有元素类型一致
                    for item in items.iter().skip(1) {
                        let t = self.infer_expr(item, st);
                        if !self.is_compatible(&elem_type, &t) {
                            self.errors.push(TypeError::new(
                                format!("列表元素类型不一致: {:?} 和 {:?}", elem_type, t),
                            ));
                        }
                    }
                    Type::List(Box::new(elem_type))
                }
            }

            Expr::Dict(entries) => {
                if entries.is_empty() {
                    Type::Dict(Box::new(Type::Any), Box::new(Type::Any))
                } else {
                    let k_type = self.infer_expr(&entries[0].0, st);
                    let v_type = self.infer_expr(&entries[0].1, st);
                    for (k, v) in entries.iter().skip(1) {
                        let kt = self.infer_expr(k, st);
                        let vt = self.infer_expr(v, st);
                        if !self.is_compatible(&k_type, &kt) {
                            self.errors.push(TypeError::new("字典键类型不一致"));
                        }
                        if !self.is_compatible(&v_type, &vt) {
                            self.errors.push(TypeError::new("字典值类型不一致"));
                        }
                    }
                    Type::Dict(Box::new(k_type), Box::new(v_type))
                }
            }

            Expr::Tuple(items) => {
                let types: Vec<Type> = items.iter().map(|i| self.infer_expr(i, st)).collect();
                Type::Tuple(types)
            }

            Expr::Index { target, index } => {
                let target_type = self.infer_expr(target, st);
                let index_type = self.infer_expr(index, st);
                match &target_type {
                    Type::List(elem) => {
                        if !self.is_compatible(&index_type, &Type::Int) {
                            self.errors.push(TypeError::new("列表索引必须为 int"));
                        }
                        *elem.clone()
                    }
                    Type::Dict(_, v_type) => {
                        *v_type.clone()
                    }
                    Type::Str => {
                        if !self.is_compatible(&index_type, &Type::Int) {
                            self.errors.push(TypeError::new("字符串索引必须为 int"));
                        }
                        Type::Str
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("类型 {:?} 不支持索引操作", target_type),
                        ));
                        Type::Any
                    }
                }
            }

            Expr::Dot { target, field } => {
                let _target_type = self.infer_expr(target, st);
                // 字段访问在 Phase 2 中实现（结构体/枚举字段）
                // 当前返回 Any
                Type::Any
            }

            Expr::Dollar => {
                // `$` 的类型在运行时确定，编译期标记为 Any
                Type::Any
            }

            Expr::DollarKey(key) => {
                let _ = key;
                Type::Any
            }

            Expr::CrossRef { domain, name } => {
                let full = format!("{}::{}", domain, name);
                if let Some(sym) = st.lookup(&full) {
                    sym.resolved_type.clone().unwrap_or(Type::Any)
                } else if let Some(sym) = st.lookup(name) {
                    sym.resolved_type.clone().unwrap_or(Type::Any)
                } else {
                    self.errors.push(TypeError::new(format!("未定义的跨域引用 `{full}`")));
                    Type::Any
                }
            }

            Expr::DoFn { params, ret_type, body: _ } => {
                let _ = params;
                let _ = ret_type;
                Type::Any // 匿名函数类型在更完整的分析中处理
            }

            Expr::Call { name, args } => {
                // 函数调用表达式
                let _arg_types: Vec<Type> = args.iter().map(|a| self.infer_expr(a, st)).collect();
                // 查函数返回类型
                if let Some(sym) = st.lookup(name) {
                    sym.resolved_type.clone().unwrap_or(Type::Any)
                } else {
                    self.errors.push(TypeError::new(format!("未定义的函数 `{name}`")));
                    Type::Any
                }
            }
        }
    }

    // ── 二元运算类型推导 ─────────────────────────

    fn check_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, st: &SymbolTable) -> Type {
        let l = self.infer_expr(lhs, st);
        let r = self.infer_expr(rhs, st);

        match op {
            // 算术运算
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                match (&l, &r) {
                    (Type::Int, Type::Int) => Type::Int,
                    (Type::Float, Type::Float) => Type::Float,
                    (Type::Int, Type::Float) | (Type::Float, Type::Int) => Type::Float,
                    (Type::Str, Type::Str) if op == BinOp::Add => Type::Str, // str + str
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("类型不兼容: {:?} {:?} {:?}", l, op, r),
                        ));
                        Type::Any
                    }
                }
            }
            // 比较运算 → bool
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                if self.is_compatible(&l, &r) {
                    Type::Bool
                } else {
                    self.errors.push(TypeError::new(
                        format!("无法比较类型 {:?} 和 {:?}", l, r),
                    ));
                    Type::Bool // 仍返回 bool 以继续分析
                }
            }
            // 逻辑运算 → bool
            BinOp::And | BinOp::Or => {
                if l == Type::Bool && r == Type::Bool {
                    Type::Bool
                } else {
                    self.errors.push(TypeError::new(
                        format!("逻辑运算要求 bool 类型，得到 {:?} 和 {:?}", l, r),
                    ));
                    Type::Bool
                }
            }
            // 位运算 → int
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                if l == Type::Int && r == Type::Int {
                    Type::Int
                } else {
                    self.errors.push(TypeError::new(
                        format!("位运算要求 int 类型，得到 {:?} 和 {:?}", l, r),
                    ));
                    Type::Int
                }
            }
        }
    }

    // ── 一元运算类型推导 ─────────────────────────

    fn check_unary(&mut self, op: UnOp, expr: &Expr, st: &SymbolTable) -> Type {
        let inner = self.infer_expr(expr, st);
        match op {
            UnOp::Neg => {
                if inner == Type::Int || inner == Type::Float {
                    inner
                } else {
                    self.errors.push(TypeError::new(
                        format!("负号要求 int/float，得到 {:?}", inner),
                    ));
                    Type::Int
                }
            }
            UnOp::Not => {
                if inner == Type::Bool {
                    Type::Bool
                } else {
                    self.errors.push(TypeError::new(
                        format!("not 要求 bool，得到 {:?}", inner),
                    ));
                    Type::Bool
                }
            }
            UnOp::BitNot => {
                if inner == Type::Int {
                    Type::Int
                } else {
                    self.errors.push(TypeError::new(
                        format!("~ 要求 int，得到 {:?}", inner),
                    ));
                    Type::Int
                }
            }
        }
    }

    // ── 类型兼容性 ───────────────────────────────

    /// 检查两种类型是否兼容（赋值、比较、运算）。
    /// int 可以隐式提升为 float。
    pub fn is_compatible(&self, a: &Type, b: &Type) -> bool {
        if a == b {
            return true;
        }
        // int → float 隐式提升
        if *a == Type::Int && *b == Type::Float {
            return true;
        }
        if *a == Type::Float && *b == Type::Int {
            return true;
        }
        // Any 与任何类型兼容（推导失败标记）
        if matches!(a, Type::Any) || matches!(b, Type::Any) {
            return true;
        }
        false
    }

    /// 检查值类型是否匹配标注类型（标注兼容值）。
    pub fn check_annotation(&mut self, ann: &Type, value: &Type, name: &str) {
        if !self.is_compatible(ann, value) && *value != Type::Any {
            self.errors.push(TypeError::new(
                format!("变量 `{name}` 标注为 {:?}，但值类型为 {:?}", ann, value),
            ));
        }
    }

    /// 检查函数返回类型是否匹配。
    pub fn check_return(&mut self, expected: &Type, actual: &Type, fn_name: &str) {
        if !self.is_compatible(expected, actual) && *actual != Type::Any {
            self.errors.push(TypeError::new(
                format!("函数 `{fn_name}` 返回类型应为 {:?}，实际为 {:?}", expected, actual),
            ));
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ast::*;
    use crate::compiler::symbol::*;

    fn new_st() -> SymbolTable {
        SymbolTable::new()
    }

    #[test]
    fn literal_types() {
        let mut tc = TypeChecker::new();
        let st = new_st();
        assert_eq!(tc.infer_expr(&Expr::Int(42), &st), Type::Int);
        assert_eq!(tc.infer_expr(&Expr::Float(3.14), &st), Type::Float);
        assert_eq!(tc.infer_expr(&Expr::Str("hi".into()), &st), Type::Str);
        assert_eq!(tc.infer_expr(&Expr::Bool(true), &st), Type::Bool);
    }

    #[test]
    fn binary_arithmetic() {
        let mut tc = TypeChecker::new();
        let st = new_st();
        // 42 + 3.14 → float
        let expr = Expr::Binary {
            op: BinOp::Add,
            lhs: Box::new(Expr::Int(42)),
            rhs: Box::new(Expr::Float(3.14)),
        };
        assert_eq!(tc.infer_expr(&expr, &st), Type::Float);
        assert!(tc.errors.is_empty());
    }

    #[test]
    fn binary_type_error() {
        let mut tc = TypeChecker::new();
        let st = new_st();
        // 42 + true → error
        let expr = Expr::Binary {
            op: BinOp::Add,
            lhs: Box::new(Expr::Int(42)),
            rhs: Box::new(Expr::Bool(true)),
        };
        tc.infer_expr(&expr, &st);
        assert!(!tc.errors.is_empty());
    }

    #[test]
    fn string_concat() {
        let mut tc = TypeChecker::new();
        let st = new_st();
        let expr = Expr::Binary {
            op: BinOp::Add,
            lhs: Box::new(Expr::Str("hello".into())),
            rhs: Box::new(Expr::Str(" world".into())),
        };
        assert_eq!(tc.infer_expr(&expr, &st), Type::Str);
    }

    #[test]
    fn logical_ops() {
        let mut tc = TypeChecker::new();
        let st = new_st();
        let expr = Expr::Binary {
            op: BinOp::And,
            lhs: Box::new(Expr::Bool(true)),
            rhs: Box::new(Expr::Bool(false)),
        };
        assert_eq!(tc.infer_expr(&expr, &st), Type::Bool);
    }

    #[test]
    fn list_type() {
        let mut tc = TypeChecker::new();
        let st = new_st();
        let expr = Expr::List(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]);
        assert_eq!(
            tc.infer_expr(&expr, &st),
            Type::List(Box::new(Type::Int))
        );
    }

    #[test]
    fn undefined_variable() {
        let mut tc = TypeChecker::new();
        let st = new_st();
        tc.infer_expr(&Expr::Ident("undefined".into()), &st);
        assert!(!tc.errors.is_empty());
    }

    #[test]
    fn defined_variable() {
        let mut tc = TypeChecker::new();
        let mut st = new_st();
        st.declare(
            Symbol::new("x".into(), SymbolKind::Variable)
                .with_type(Type::Int),
        )
        .unwrap();
        assert_eq!(tc.infer_expr(&Expr::Ident("x".into()), &st), Type::Int);
        assert!(tc.errors.is_empty());
    }

    #[test]
    fn annotation_check() {
        let mut tc = TypeChecker::new();
        tc.check_annotation(&Type::Int, &Type::Float, "x");
        // int 可提升为 float → 兼容
        assert!(tc.errors.is_empty());
    }

    #[test]
    fn annotation_mismatch() {
        let mut tc = TypeChecker::new();
        tc.check_annotation(&Type::Int, &Type::Str, "x");
        assert!(!tc.errors.is_empty());
    }

    #[test]
    fn int_to_float_compatible() {
        let tc = TypeChecker::new();
        assert!(tc.is_compatible(&Type::Int, &Type::Float));
        assert!(tc.is_compatible(&Type::Float, &Type::Int));
        assert!(!tc.is_compatible(&Type::Int, &Type::Str));
    }
}
