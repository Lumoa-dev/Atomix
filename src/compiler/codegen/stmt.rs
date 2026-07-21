//! 语句编译 — AST 语句 → IR 指令序列。
//!
//! 覆盖 04-编译管线.md §5.2 的语句到 IR 映射规则。

use crate::base::isa::{opcode, reg};
use crate::compiler::ast::{Stmt, TryFilter};
use crate::compiler::codegen::assembly::ExnEntry;
use crate::compiler::codegen::expr::{ConstPool, compile_expr};
use crate::compiler::codegen::instr::{InstrEmitter, VReg, vreg_to_preg};

/// 编译语句列表。
pub fn compile_stmts(
    emit: &mut InstrEmitter,
    pool: &mut ConstPool,
    stmts: &[Stmt],
    exn_entries: &mut Vec<ExnEntry>,
) {
    // 收集所有函数定义的标签
    for stmt in stmts {
        if let Stmt::FnDef { def: f, .. } = stmt {
            emit.emit_label(&f.name);
        }
    }

    for stmt in stmts {
        compile_stmt(emit, pool, stmt, exn_entries);
    }
}

/// 编译单条语句。
pub fn compile_stmt(
    emit: &mut InstrEmitter,
    pool: &mut ConstPool,
    stmt: &Stmt,
    exn_entries: &mut Vec<ExnEntry>,
) {
    // 记录语句行号（供 .debug 段和运行时错误报告使用）
    let line = match stmt {
        Stmt::Let { line, .. }
        | Stmt::Const { line, .. }
        | Stmt::Goout { line, .. }
        | Stmt::Call { line, .. }
        | Stmt::Wait { line, .. }
        | Stmt::If { line, .. }
        | Stmt::For { line, .. }
        | Stmt::Break { line, .. }
        | Stmt::Continue { line, .. }
        | Stmt::Assert { line, .. }
        | Stmt::Raise { line, .. }
        | Stmt::Return { line, .. }
        | Stmt::Block { line, .. }
        | Stmt::FnDef { line, .. } => *line,
    };
    emit.set_source_line(line as u32);

    match stmt {
        Stmt::Let { init, .. } | Stmt::Const { init, .. } | Stmt::Goout { init, .. } => {
            compile_expr(emit, pool, init);
        }

        Stmt::Call {
            func_name,
            args,
            output,
            pipe,
            try_handler,
            ..
        } => {
            // 检查是否为内置函数（编译期 IR 展开）
            if let Some(builtin) = crate::compiler::builtins::lookup(func_name) {
                // 编译参数到虚拟寄存器
                let arg_vregs: Vec<VReg> = args
                    .iter()
                    .map(|arg| compile_expr(emit, pool, arg))
                    .collect();
                // 展开为 IR 指令序列
                (builtin.expand)(emit, &arg_vregs);
                let _ = output;
                let _ = pipe;
                let _ = try_handler;
                return;
            }

            // 参数传入 R4-R7
            for (i, arg) in args.iter().enumerate() {
                if i >= 4 {
                    break;
                }
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

            // TRY 保护区域起点
            let try_start = try_handler.as_ref().map(|_| emit.instr_count() as u32);
            emit.emit_ji(opcode::CALL, 0);

            // 如果有 TRY handler，生成 .exn 条目和 handler 代码
            if let Some(handler) = try_handler {
                let end_pc = emit.instr_count() as u32; // CALL 之后
                let handler_idx = emit.instr_count();

                // 跳过 handler（正常路径）
                let skip_label = format!(".L_skip_try_{}", handler_idx);
                emit.emit_jmp_to(&skip_label);

                // Handler 代码
                let handler_label = format!(".L_try_handler_{}", handler_idx);
                emit.emit_label(&handler_label);
                compile_stmts(emit, pool, &handler.body, exn_entries);

                // 正常路径汇合点
                emit.emit_label(&skip_label);
                emit.resolve_all();

                let filter = match &handler.filter {
                    TryFilter::All => 0u16,
                    TryFilter::IsError(_) => 1u16,
                    TryFilter::IsTimeout(_) => 2u16,
                };

                exn_entries.push(ExnEntry {
                    start_pc: try_start.unwrap_or(0),
                    end_pc,
                    handler_pc: handler_idx as u32,
                    filter,
                });
            }

            // 返回值从 R4 取出
            let result_reg = vreg_to_preg(4); // 用临时寄存器
            emit.emit_mov(result_reg, reg::A0 as u8);
            let _ = output;
            let _ = pipe;
        }

        Stmt::Wait {
            template,
            overrides,
            try_handler,
            ..
        } => {
            // WAIT → TASK_FORK + TASK_JOIN
            for (name, val) in overrides {
                let vreg = compile_expr(emit, pool, val);
                emit.emit_mov(reg::A0 as u8, vreg_to_preg(vreg));
                let _ = name;
            }
            emit.emit_label(&format!("fork_{}", template));

            // TRY 保护区域起点（覆盖 TASK_FORK + TASK_JOIN）
            let try_start = try_handler.as_ref().map(|_| emit.instr_count() as u32);
            emit.emit_r1i(opcode::TASK_FORK, reg::T0 as u8, 0);
            emit.emit_r2i(opcode::TASK_JOIN, reg::T1 as u8, reg::T0 as u8, 0);

            // 如果有 TRY handler，生成 .exn 条目和 handler 代码
            if let Some(handler) = try_handler {
                let end_pc = emit.instr_count() as u32;
                let handler_idx = emit.instr_count();

                let skip_label = format!(".L_skip_try_w_{}", handler_idx);
                emit.emit_jmp_to(&skip_label);

                let handler_label = format!(".L_try_handler_w_{}", handler_idx);
                emit.emit_label(&handler_label);
                compile_stmts(emit, pool, &handler.body, exn_entries);

                emit.emit_label(&skip_label);
                emit.resolve_all();

                let filter = match &handler.filter {
                    TryFilter::All => 0u16,
                    TryFilter::IsError(_) => 1u16,
                    TryFilter::IsTimeout(_) => 2u16,
                };

                exn_entries.push(ExnEntry {
                    start_pc: try_start.unwrap_or(0),
                    end_pc,
                    handler_pc: handler_idx as u32,
                    filter,
                });
            }
        }

        Stmt::If {
            cond,
            body,
            elifs,
            else_body,
            ..
        } => {
            let endif_label = format!(".L_end_if_{}", emit.instr_count());

            // IF 分支
            let cond_vreg = compile_expr(emit, pool, cond);
            let else_label = if elifs.is_empty() && else_body.is_none() {
                endif_label.clone()
            } else {
                format!(".L_else_{}", emit.instr_count())
            };
            emit.emit_jz_to(vreg_to_preg(cond_vreg), &else_label);
            compile_stmts(emit, pool, body, exn_entries);
            emit.emit_jmp_to(&endif_label);

            // ELIF 分支
            let mut current_else = else_label;
            for (elif_cond, elif_body) in elifs {
                emit.emit_label(&current_else);
                let elif_vreg = compile_expr(emit, pool, elif_cond);
                let next_label = format!(".L_next_{}", emit.instr_count());
                emit.emit_jz_to(vreg_to_preg(elif_vreg), &next_label);
                compile_stmts(emit, pool, elif_body, exn_entries);
                emit.emit_jmp_to(&endif_label);
                current_else = next_label;
            }

            // ELSE 分支
            if let Some(eb) = else_body {
                emit.emit_label(&current_else);
                compile_stmts(emit, pool, eb, exn_entries);
            }

            emit.emit_label(&endif_label);
            // 解析前向引用
            emit.resolve_all();
        }

        Stmt::For { cond, body, .. } => {
            let loop_label = format!(".L_loop_{}", emit.instr_count());
            let exit_label = format!(".L_exit_{}", emit.instr_count());

            emit.emit_label(&loop_label);
            let cond_vreg = compile_expr(emit, pool, cond);
            emit.emit_jz_to(vreg_to_preg(cond_vreg), &exit_label);
            compile_stmts(emit, pool, body, exn_entries);
            emit.emit_jmp_to(&loop_label);
            emit.emit_label(&exit_label);
            emit.resolve_all();
        }

        Stmt::Break { cond, .. } => {
            // BREAK cond → 如果 cond 成立则跳转到循环出口
            let exit_label = format!(".L_exit_{}", emit.instr_count());
            if let Some(c) = cond {
                let vreg = compile_expr(emit, pool, c);
                emit.emit_jnz_to(vreg_to_preg(vreg), &exit_label);
            }
            emit.emit_jmp_to(&exit_label);
            emit.resolve_all();
        }

        Stmt::Continue { cond, .. } => {
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

        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                let vreg = compile_expr(emit, pool, v);
                emit.emit_mov(reg::A0 as u8, vreg_to_preg(vreg));
            }
            emit.emit_r1i(opcode::JMPR, reg::RA as u8, 0);
        }

        Stmt::Block { stmts, .. } => {
            compile_stmts(emit, pool, stmts, exn_entries);
        }

        Stmt::FnDef { .. } => {
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
        let mut exn = Vec::new();
        compile_stmt(&mut emit, &mut pool, &stmt, &mut exn);
        emit.resolve_all();
        (emit.text, pool)
    }

    fn compile_many(stmts: Vec<Stmt>) -> (Vec<u32>, ConstPool) {
        reset_vreg();
        let mut emit = InstrEmitter::new();
        let mut pool = ConstPool::new();
        let mut exn = Vec::new();
        compile_stmts(&mut emit, &mut pool, &stmts, &mut exn);
        emit.resolve_all();
        (emit.text, pool)
    }

    #[test]
    fn let_int() {
        let stmt = Stmt::Let {
            line: 0,
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
            line: 0,
            cond: Expr::Bool(true),
            body: vec![Stmt::Let {
                line: 0,
                name: "x".into(),
                type_ann: TypeNode::Base("int".into()),
                init: Expr::Int(1),
            }],
            elifs: Vec::new(),
            else_body: Some(vec![Stmt::Let {
                line: 0,
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
            line: 0,
            cond: Expr::Bool(true),
            body: vec![],
        };
        let (text, _) = compile_one(stmt);
        assert!(text.len() >= 2);
    }

    #[test]
    fn return_int() {
        let stmt = Stmt::Return {
            line: 0,
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
            line: 0,
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
            line: 0,
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
            line: 0,
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
                line: 0,
                name: "a".into(),
                type_ann: TypeNode::Base("int".into()),
                init: Expr::Int(1),
            },
            Stmt::Let {
                line: 0,
                name: "b".into(),
                type_ann: TypeNode::Base("int".into()),
                init: Expr::Int(2),
            },
        ];
        let (text, _) = compile_many(stmts);
        assert_eq!(text.len(), 2); // two MOVI
    }
}
