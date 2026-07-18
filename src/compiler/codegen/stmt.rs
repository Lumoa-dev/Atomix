//! 语句编译 — AST 语句 → IR 指令序列。
//!
//! 覆盖 04-编译管线.md §5.2 的语句到 IR 映射规则。

use crate::base::isa::{self, opcode, reg};
use crate::compiler::ast::Stmt;
use crate::compiler::codegen::expr::{compile_expr, reset_vreg, ConstPool};
use crate::compiler::codegen::instr::{InstrEmitter, vreg_to_preg};

/// 编译语句列表。
pub fn compile_stmts(emit: &mut InstrEmitter, pool: &mut ConstPool, stmts: &[Stmt]) {
    // 收集所有函数定义的标签
    for stmt in stmts {
        match stmt {
            Stmt::FnDef(f) => {
                emit.emit_label(&f.name);
            }
            _ => {}
        }
    }

    for stmt in stmts {
        compile_stmt(emit, pool, stmt);
    }
}

/// 编译单条语句。
pub fn compile_stmt(emit: &mut InstrEmitter, pool: &mut ConstPool, stmt: &Stmt) {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Const { init, .. } | Stmt::Goout { init, .. } => {
            compile_expr(emit, pool, init);
        }

        Stmt::Call { func_name, args, output, pipe, .. } => {
            // 参数传入 R4-R7
            for (i, arg) in args.iter().enumerate() {
                if i >= 4 { break; }
                let vreg = compile_expr(emit, pool, arg);
                let preg = match i {
                    0 => reg::A0 as u8,
                    1 => reg::A1 as u8,
                    2 => reg::A2 as u8,
                    3 => reg::A3 as u8,
                    _ => unreachable!(),
                };
                emit.emit_mov(preg, vreg_to_preg(vreg));
            }
            // CALL 指令（偏移在链接阶段填充）
            emit.emit_label(&format!("call_{}", func_name));
            emit.emit_ji(opcode::CALL, 0);
            // 返回值从 R4 取出
            let result_reg = vreg_to_preg(4); // 用临时寄存器
            emit.emit_mov(result_reg, reg::A0 as u8);
            let _ = output;
            let _ = pipe;
        }

        Stmt::Wait { template, overrides, .. } => {
            // WAIT → TASK_FORK + TASK_JOIN
            for (name, val) in overrides {
                let vreg = compile_expr(emit, pool, val);
                emit.emit_mov(reg::A0 as u8, vreg_to_preg(vreg));
                let _ = name;
            }
            emit.emit_label(&format!("fork_{}", template));
            emit.emit_r1i(opcode::TASK_FORK, reg::T0 as u8, 0);
            emit.emit_r2i(opcode::TASK_JOIN, reg::T1 as u8, reg::T0 as u8, 0);
        }

        Stmt::If { cond, body, elifs, else_body } => {
            let endif_label = format!(".L_end_if_{}", emit.instr_count());

            // IF 分支
            let cond_vreg = compile_expr(emit, pool, cond);
            let else_label = if elifs.is_empty() && else_body.is_none() {
                endif_label.clone()
            } else {
                format!(".L_else_{}", emit.instr_count())
            };
            emit.emit_jz_to(vreg_to_preg(cond_vreg), &else_label);
            compile_stmts(emit, pool, body);
            emit.emit_jmp_to(&endif_label);

            // ELIF 分支
            let mut current_else = else_label;
            for (elif_cond, elif_body) in elifs {
                emit.emit_label(&current_else);
                let elif_vreg = compile_expr(emit, pool, elif_cond);
                let next_label = format!(".L_next_{}", emit.instr_count());
                emit.emit_jz_to(vreg_to_preg(elif_vreg), &next_label);
                compile_stmts(emit, pool, elif_body);
                emit.emit_jmp_to(&endif_label);
                current_else = next_label;
            }

            // ELSE 分支
            if let Some(eb) = else_body {
                emit.emit_label(&current_else);
                compile_stmts(emit, pool, eb);
            }

            emit.emit_label(&endif_label);
            // 解析前向引用
            emit.resolve_all();
        }

        Stmt::For { cond, body } => {
            let loop_label = format!(".L_loop_{}", emit.instr_count());
            let exit_label = format!(".L_exit_{}", emit.instr_count());

            emit.emit_label(&loop_label);
            let cond_vreg = compile_expr(emit, pool, cond);
            emit.emit_jz_to(vreg_to_preg(cond_vreg), &exit_label);
            compile_stmts(emit, pool, body);
            emit.emit_jmp_to(&loop_label);
            emit.emit_label(&exit_label);
            emit.resolve_all();
        }

        Stmt::Break { cond } => {
            // BREAK cond → 如果 cond 成立则跳转到循环出口
            let exit_label = format!(".L_exit_{}", emit.instr_count());
            if let Some(c) = cond {
                let vreg = compile_expr(emit, pool, c);
                emit.emit_jnz_to(vreg_to_preg(vreg), &exit_label);
            }
            emit.emit_jmp_to(&exit_label);
            emit.resolve_all();
        }

        Stmt::Continue { cond } => {
            let loop_label = format!(".L_loop_{}", emit.instr_count());
            if let Some(c) = cond {
                let vreg = compile_expr(emit, pool, c);
                emit.emit_jnz_to(vreg_to_preg(vreg), &loop_label);
            }
            emit.emit_jmp_to(&loop_label);
            emit.resolve_all();
        }

        Stmt::Assert { cond, .. } => {
            let vreg = compile_expr(emit, pool, cond);
            let ok_label = format!(".L_assert_ok_{}", emit.instr_count());
            emit.emit_jnz_to(vreg_to_preg(vreg), &ok_label);
            // ASSERT 失败：THROW
            emit.emit_movi(reg::A0 as u8, 1);
            emit.emit_r1i(opcode::THROW, reg::A0 as u8, 0);
            emit.emit_label(&ok_label);
            emit.resolve_all();
        }

        Stmt::Raise { expr, .. } => {
            let vreg = compile_expr(emit, pool, expr);
            emit.emit_mov(reg::A0 as u8, vreg_to_preg(vreg));
            emit.emit_r1i(opcode::THROW, reg::A0 as u8, 0);
        }

        Stmt::Return { value } => {
            if let Some(v) = value {
                let vreg = compile_expr(emit, pool, v);
                emit.emit_mov(reg::A0 as u8, vreg_to_preg(vreg));
            }
            emit.emit_r1i(opcode::JMPR, reg::RA as u8, 0);
        }

        Stmt::Block(stmts) => {
            compile_stmts(emit, pool, stmts);
        }

        Stmt::FnDef(_) => {
            // 函数定义已在标签收集阶段处理
        }
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ast::*;
    use crate::compiler::codegen::expr::reset_vreg;

    fn compile_one(stmt: Stmt) -> (Vec<u32>, ConstPool) {
        reset_vreg();
        let mut emit = InstrEmitter::new();
        let mut pool = ConstPool::new();
        compile_stmt(&mut emit, &mut pool, &stmt);
        emit.resolve_all();
        (emit.text, pool)
    }

    fn compile_many(stmts: Vec<Stmt>) -> (Vec<u32>, ConstPool) {
        reset_vreg();
        let mut emit = InstrEmitter::new();
        let mut pool = ConstPool::new();
        compile_stmts(&mut emit, &mut pool, &stmts);
        emit.resolve_all();
        (emit.text, pool)
    }

    #[test]
    fn let_int() {
        let stmt = Stmt::Let {
            name: "x".into(),
            type_ann: TypeNode::Base("int".into()),
            init: Expr::Int(42),
        };
        let (text, _) = compile_one(stmt);
        assert!(!text.is_empty());
        assert_eq!((text[0] >> 24) as u8, opcode::MOVI);
    }

    #[test]
    fn if_else_pattern() {
        let stmt = Stmt::If {
            cond: Expr::Bool(true),
            body: vec![Stmt::Let {
                name: "x".into(),
                type_ann: TypeNode::Base("int".into()),
                init: Expr::Int(1),
            }],
            elifs: Vec::new(),
            else_body: Some(vec![Stmt::Let {
                name: "x".into(),
                type_ann: TypeNode::Base("int".into()),
                init: Expr::Int(2),
            }]),
        };
        let (text, _) = compile_one(stmt);
        // Pattern: JZ → body → JMP → else → endif
        assert!(text.len() >= 4);
    }

    #[test]
    fn for_loop_emits_instructions() {
        let stmt = Stmt::For {
            cond: Expr::Bool(true),
            body: vec![],
        };
        let (text, _) = compile_one(stmt);
        assert!(text.len() >= 2);
    }

    #[test]
    fn return_int() {
        let stmt = Stmt::Return {
            value: Some(Expr::Int(42)),
        };
        let (text, _) = compile_one(stmt);
        // MOVI t0, 42; MOV a0, t0; JMPR ra
        assert_eq!((text[0] >> 24) as u8, opcode::MOVI);
        assert_eq!((text[2] >> 24) as u8, opcode::JMPR);
    }

    #[test]
    fn call_function() {
        let stmt = Stmt::Call {
            input: None,
            func_name: "foo".into(),
            args: vec![Expr::Int(1), Expr::Int(2)],
            output: None,
            pipe: false,
            try_handler: None,
        };
        let (text, _) = compile_one(stmt);
        // MOVI t0, 1; MOV a0, t0; MOVI t1, 2; MOV a1, t1; CALL
        assert_eq!((text[0] >> 24) as u8, opcode::MOVI);
        assert_eq!((text[4] >> 24) as u8, opcode::CALL);
    }

    #[test]
    fn assert_ok_path() {
        let stmt = Stmt::Assert {
            cond: Expr::Bool(true),
            msg: None,
        };
        let (text, _) = compile_one(stmt);
        // MOVI + JNZ(跳过THROW) + THROW(不会到达)
        assert!(!text.is_empty());
    }

    #[test]
    fn raise_exception() {
        let stmt = Stmt::Raise {
            expr: Expr::Int(1),
            msg: None,
        };
        let (text, _) = compile_one(stmt);
        // MOVI t0, 1; MOV a0, t0; THROW a0
        assert!(!text.is_empty());
        assert_eq!((text[0] >> 24) as u8, opcode::MOVI);
    }

    #[test]
    fn multiple_let_statement() {
        let stmts = vec![
            Stmt::Let {
                name: "a".into(),
                type_ann: TypeNode::Base("int".into()),
                init: Expr::Int(1),
            },
            Stmt::Let {
                name: "b".into(),
                type_ann: TypeNode::Base("int".into()),
                init: Expr::Int(2),
            },
        ];
        let (text, _) = compile_many(stmts);
        assert_eq!(text.len(), 2); // two MOVI
    }
}
