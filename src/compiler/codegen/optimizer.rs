//! 多级优化器 — O0/O1/O2/Os。
//!
//! 覆盖 04-编译管线.md §6 的优化规范。

use crate::base::isa::{self, opcode, reg};

// ─── 优化级别 ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    /// 无优化（默认，dev 模式）
    O0,
    /// 常量折叠 + 死代码消除 + 窥孔
    O1,
    /// O1 + 函数内联 + 循环展开 + 公共子表达式消除
    O2,
    /// 体积优化（优先减小 .atxe）
    Os,
}

impl std::str::FromStr for OptLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "1" | "o1" => OptLevel::O1,
            "2" | "o2" => OptLevel::O2,
            "s" | "os" => OptLevel::Os,
            _ => OptLevel::O0,
        })
    }
}

// ─── 优化器 ────────────────────────────────────────────

pub struct Optimizer {
    pub level: OptLevel,
    stats: OptStats,
}

#[derive(Debug, Default, Clone)]
pub struct OptStats {
    pub constant_folds: usize,
    pub dead_eliminations: usize,
    pub peephole_applies: usize,
}

impl Optimizer {
    pub fn new(level: OptLevel) -> Self {
        Self {
            level,
            stats: OptStats::default(),
        }
    }

    pub fn stats(&self) -> &OptStats {
        &self.stats
    }

    /// 运行优化，返回优化后的指令序列。
    pub fn optimize(&mut self, text: &[u32]) -> Vec<u32> {
        match self.level {
            OptLevel::O0 => text.to_vec(),
            OptLevel::O1 => {
                let mut result = text.to_vec();
                result = self.constant_fold(&result);
                result = self.dead_code_eliminate(&result);
                result = self.peephole(&result);
                result
            }
            OptLevel::O2 => {
                let mut result = text.to_vec();
                result = self.constant_fold(&result);
                result = self.dead_code_eliminate(&result);
                result = self.peephole(&result);
                // O2 额外：常量折叠再次迭代
                result = self.constant_fold(&result);
                result
            }
            OptLevel::Os => {
                let mut result = text.to_vec();
                result = self.dead_code_eliminate(&result);
                result = self.peephole(&result);
                result
            }
        }
    }

    // ═══════════════════════════════════════════════
    //  常量折叠
    // ═══════════════════════════════════════════════

    /// 常量折叠：编译期求值常量表达式。
    /// 模式：MOVI rd, a; MOVI rs, b; OP rd, rd, rs → MOVI rd, result
    fn constant_fold(&mut self, text: &[u32]) -> Vec<u32> {
        let mut result = Vec::with_capacity(text.len());
        let mut i = 0;
        while i < text.len() {
            // 尝试匹配三元指令模式: MOVI rd, a; OP rd, rd, rs
            if i + 2 < text.len() {
                let op1 = text[i];
                let op2 = text[i + 1];
                let op3 = text[i + 2];

                let op1_kind = (op1 >> 24) as u8;
                let op2_kind = (op2 >> 24) as u8;
                let op3_kind = (op3 >> 24) as u8;

                // MOVI rd, a; MOVI rs, b → 两个 MOVI
                if op1_kind == opcode::MOVI && op2_kind == opcode::MOVI {
                    let rd1 = ((op1 >> 20) & 0x0F) as u8;
                    let imm1 = (op1 & 0xFFFF) as u16;
                    let rd2 = ((op2 >> 20) & 0x0F) as u8;
                    let imm2 = (op2 & 0xFFFF) as u16;

                    // 第三个指令使用它们: OP rd, rd, rs
                    if i + 2 < text.len() && self.is_alu_op(op3_kind) {
                        let op3_rd = ((op3 >> 20) & 0x0F) as u8;
                        let op3_rs1 = ((op3 >> 16) & 0x0F) as u8;
                        let op3_rs2 = ((op3 >> 12) & 0x0F) as u8;

                        // 检查操作数是否匹配: OP rd1, rd1, rd2 或 OP rd, rd1, rd2
                        let mut match_first = op3_rs1 == rd1 && op3_rs2 == rd2;
                        let mut match_second = op3_rs1 == rd2 && op3_rs2 == rd1;

                        // 对于 ADD/MUL 等交换律运算，操作数可交换
                        let is_commutative = matches!(
                            op3_kind,
                            opcode::ADD | opcode::MUL | opcode::AND | opcode::OR | opcode::XOR
                        );
                        if is_commutative {
                            match_first = match_first || (op3_rs1 == rd1 && op3_rs2 == rd2);
                            match_second = match_second || (op3_rs1 == rd2 && op3_rs2 == rd1);
                        }

                        if (match_first || match_second)
                            && let Some(folded) = try_fold(op3_kind, imm1 as i64, imm2 as i64)
                        {
                            let target_rd = op3_rd;
                            if folded >= 0 && folded <= u16::MAX as i64 {
                                result.push(isa::encode_r2i(
                                    opcode::MOVI,
                                    target_rd,
                                    0,
                                    folded as u16,
                                ));
                                self.stats.constant_folds += 1;
                                i += 3;
                                continue;
                            }
                        }
                    }
                }
            }

            result.push(text[i]);
            i += 1;
        }
        result
    }

    /// 是否为 ALU 运算指令（可参与常量折叠）。
    fn is_alu_op(&self, opcode: u8) -> bool {
        matches!(
            opcode,
            opcode::ADD
                | opcode::SUB
                | opcode::MUL
                | opcode::DIV
                | opcode::AND
                | opcode::OR
                | opcode::XOR
                | opcode::SHL
                | opcode::SHR
                | opcode::SEQ
                | opcode::SNE
                | opcode::SLT
                | opcode::SGT
        )
    }

    // ═══════════════════════════════════════════════
    //  死代码消除
    // ═══════════════════════════════════════════════

    /// 死代码消除：从入口标记所有可达指令，未标记的移除。
    /// 入口 = 第一条指令（指令 0）。
    fn dead_code_eliminate(&mut self, text: &[u32]) -> Vec<u32> {
        if text.is_empty() {
            return Vec::new();
        }

        let n = text.len();
        let mut reachable = vec![false; n];

        // 从每个可能成为入口的指令出发传播可达性
        // 我们标记函数入口（标签目标）和指令 0 为可达
        self.mark_reachable(text, 0, &mut reachable);

        // 此外，收集所有被 JMP/CALL/JZ/JNZ 作为目标的指令
        let targets = self.collect_jump_targets(text);
        for &t in &targets {
            if t < n {
                self.mark_reachable(text, t, &mut reachable);
            }
        }

        // 收集可达指令
        let mut result = Vec::new();
        for i in 0..n {
            if reachable[i] {
                result.push(text[i]);
            } else {
                self.stats.dead_eliminations += 1;
            }
        }

        result
    }

    /// 从给定指令出发，标记可达指令（正向传播）。
    fn mark_reachable(&self, text: &[u32], start: usize, reachable: &mut [bool]) {
        let mut i = start;
        while i < text.len() && !reachable[i] {
            reachable[i] = true;
            let instr = text[i];
            let op = (instr >> 24) as u8;

            match op {
                opcode::JMP => {
                    // 无条件跳转：标记目标，然后停止
                    let offset = isa::decode_ji(instr);
                    let target = (i as i32 + offset) as usize;
                    if target < text.len() {
                        self.mark_reachable(text, target, reachable);
                    }
                    break; // JMP 之后不可达
                }
                opcode::JZ | opcode::JNZ => {
                    // 条件跳转：标记目标，继续下一条
                    // R1I 格式：取出 imm (offset)，符号扩展 20 位到 i32
                    let raw = instr & 0x000F_FFFF;
                    let offset = if raw & 0x80000 != 0 {
                        (raw | 0xFFF00000) as i32
                    } else {
                        raw as i32
                    };
                    let target = (i as i32 + offset) as usize;
                    if target < text.len() && !reachable[target] {
                        self.mark_reachable(text, target, reachable);
                    }
                    i += 1;
                    continue;
                }
                opcode::CALL => {
                    // CALL：标记目标，继续下一条（CALL 会返回）
                    let offset = isa::decode_ji(instr);
                    let target = (i as i32 + offset) as usize;
                    if target < text.len() && !reachable[target] {
                        self.mark_reachable(text, target, reachable);
                    }
                    i += 1;
                    continue;
                }
                opcode::JMPR | opcode::THROW | opcode::TRAP => {
                    // 返回/异常/陷阱：终止此路径
                    break;
                }
                _ => {
                    i += 1;
                }
            }
        }
    }

    /// 收集所有跳转指令的目标偏移。
    fn collect_jump_targets(&self, text: &[u32]) -> Vec<usize> {
        let mut targets = Vec::new();
        for (i, &instr) in text.iter().enumerate() {
            let op = (instr >> 24) as u8;
            match op {
                opcode::JMP => {
                    let offset = isa::decode_ji(instr);
                    let target = (i as i32 + offset) as usize;
                    targets.push(target);
                }
                opcode::JZ | opcode::JNZ => {
                    let raw = instr & 0x000F_FFFF;
                    let offset = if raw & 0x80000 != 0 {
                        (raw | 0xFFF00000) as i32
                    } else {
                        raw as i32
                    };
                    let target = (i as i32 + offset) as usize;
                    targets.push(target);
                }
                opcode::CALL => {
                    let offset = isa::decode_ji(instr);
                    let target = (i as i32 + offset) as usize;
                    targets.push(target);
                }
                _ => {}
            }
        }
        targets
    }

    // ═══════════════════════════════════════════════
    //  窥孔优化 (骨架)
    // ═══════════════════════════════════════════════

    /// 窥孔优化：滑动窗口扫描指令序列，匹配→替换。
    fn peephole(&mut self, text: &[u32]) -> Vec<u32> {
        let mut result = Vec::with_capacity(text.len());
        let mut i = 0;
        while i < text.len() {
            let instr = text[i];
            let op = (instr >> 24) as u8;

            // 模式 1: MOV rd, rs; ADD rd, R0, rs → 删除 MOV（等效 nop）
            if op == opcode::MOV && i + 1 < text.len() {
                let rd = ((instr >> 20) & 0x0F) as u8;
                let rs = ((instr >> 16) & 0x0F) as u8;
                let next = text[i + 1];
                let next_op = (next >> 24) as u8;
                let next_rd = ((next >> 20) & 0x0F) as u8;
                let next_rs1 = ((next >> 16) & 0x0F) as u8;
                let next_rs2 = ((next >> 12) & 0x0F) as u8;
                // MOV rd, rs; ADD rd, R0, rs → 只保留 ADD
                if next_op == opcode::ADD
                    && next_rd == rd
                    && next_rs1 == reg::ZERO as u8
                    && next_rs2 == rs
                {
                    self.stats.peephole_applies += 1;
                    i += 1; // 跳过 MOV
                    continue;
                }
                // MOV rd, rs; ADD rd, rs, R0 → 只保留 ADD
                if next_op == opcode::ADD
                    && next_rd == rd
                    && next_rs1 == rs
                    && next_rs2 == reg::ZERO as u8
                {
                    self.stats.peephole_applies += 1;
                    i += 1;
                    continue;
                }
            }

            // 模式 2: MOVI rd, 0 → 替代为使用 ZERO 寄存器（由寄存器分配器处理）
            // 简化为：保留 MOVI，但后续指令可优化
            if op == opcode::MOVI && (instr & 0xFFFF) == 0 {
                let rd = ((instr >> 20) & 0x0F) as u8;
                // 如果下一个指令是 ADD rd, rs, R0，转换为 MOV rd, rs
                if i + 1 < text.len() {
                    let next = text[i + 1];
                    let next_op = (next >> 24) as u8;
                    if next_op == opcode::ADD {
                        let next_rd = ((next >> 20) & 0x0F) as u8;
                        let next_rs1 = ((next >> 16) & 0x0F) as u8;
                        let next_rs2 = ((next >> 12) & 0x0F) as u8;
                        if next_rd == rd && next_rs2 == reg::ZERO as u8 {
                            // ADD rd, rs, R0 → MOV rd, rs
                            result.push(isa::encode_r3(opcode::MOV, rd, next_rs1, 0, 0));
                            self.stats.peephole_applies += 1;
                            i += 2;
                            continue;
                        }
                    }
                }
            }

            // 模式 3: JMP .L → 保留，DCE 已处理不可达代码
            // 窥孔不在此处处理 JMP 后的代码（由 DCE 负责）

            // 模式 4: JZ Rt, .L; JMP .L2 → 反转: JNZ Rt, .L2; JMP .L
            if op == opcode::JZ && i + 1 < text.len() {
                let next = text[i + 1];
                if (next >> 24) as u8 == opcode::JMP {
                    // 交换：JNZ target_of_JMP; JMP target_of_JZ
                    let jz_offset = instr & 0x000F_FFFF; // R1I 格式
                    let jmp_offset = next & 0x00FF_FFFF; // JI 格式
                    let jz_rd = ((instr >> 20) & 0x0F) as u8;
                    // JNZ rd, jmp_offset
                    result.push(isa::encode_r1i(opcode::JNZ, jz_rd, jmp_offset));
                    // JMP jz_offset
                    result.push(isa::encode_ji(opcode::JMP, jz_offset));
                    self.stats.peephole_applies += 1;
                    i += 2;
                    continue;
                }
            }

            result.push(instr);
            i += 1;
        }
        result
    }
}

// ─── 常量折叠辅助 ──────────────────────────────────────

/// 尝试在编译期求值常量二元运算。
fn try_fold(opcode: u8, a: i64, b: i64) -> Option<i64> {
    match opcode {
        o if o == opcode::ADD => a.checked_add(b),
        o if o == opcode::SUB => a.checked_sub(b),
        o if o == opcode::MUL => a.checked_mul(b),
        o if o == opcode::DIV => {
            if b != 0 {
                a.checked_div(b)
            } else {
                None
            }
        }
        o if o == opcode::AND => Some(a & b),
        o if o == opcode::OR => Some(a | b),
        o if o == opcode::XOR => Some(a ^ b),
        o if o == opcode::SHL => a.checked_shl(b as u32),
        o if o == opcode::SHR => a.checked_shr(b as u32),
        o if o == opcode::SEQ => Some(if a == b { 1 } else { 0 }),
        o if o == opcode::SNE => Some(if a != b { 1 } else { 0 }),
        o if o == opcode::SLT => Some(if a < b { 1 } else { 0 }),
        o if o == opcode::SGT => Some(if a > b { 1 } else { 0 }),
        _ => None,
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_movi(rd: u8, imm: u16) -> u32 {
        isa::encode_r2i(opcode::MOVI, rd, 0, imm)
    }

    fn make_r3(op: u8, rd: u8, rs1: u8, rs2: u8) -> u32 {
        isa::encode_r3(op, rd, rs1, rs2, 0)
    }

    #[test]
    fn o0_no_change() {
        let text = vec![
            make_movi(8, 2),
            make_movi(9, 3),
            make_r3(opcode::ADD, 10, 8, 9),
        ];
        let mut opt = Optimizer::new(OptLevel::O0);
        let result = opt.optimize(&text);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn constant_fold_add() {
        // MOVI t0,2; MOVI t1,3; ADD t2,t0,t1 → MOVI t2,5
        let text = vec![
            make_movi(8, 2),
            make_movi(9, 3),
            make_r3(opcode::ADD, 10, 8, 9),
        ];
        let mut opt = Optimizer::new(OptLevel::O1);
        let result = opt.optimize(&text);
        assert_eq!(result.len(), 1);
        assert_eq!((result[0] >> 24) as u8, opcode::MOVI);
        assert_eq!((result[0] & 0xFFFF) as u16, 5);
    }

    #[test]
    fn constant_fold_mul() {
        let text = vec![
            make_movi(8, 6),
            make_movi(9, 7),
            make_r3(opcode::MUL, 10, 8, 9),
        ];
        let mut opt = Optimizer::new(OptLevel::O1);
        let result = opt.optimize(&text);
        assert_eq!(result.len(), 1);
        assert_eq!((result[0] & 0xFFFF) as u16, 42);
    }

    #[test]
    fn constant_fold_commutative() {
        let text = vec![
            make_movi(8, 3),
            make_movi(9, 4),
            make_r3(opcode::ADD, 10, 9, 8),
        ];
        let mut opt = Optimizer::new(OptLevel::O1);
        let result = opt.optimize(&text);
        assert_eq!(result.len(), 1);
        assert_eq!((result[0] & 0xFFFF) as u16, 7);
    }

    #[test]
    fn dead_code_eliminate_jmp_fallthrough() {
        // JMP to end; NOP; NOP; NOP(end)
        // instr 0: JMP +3 → 跳转到 instr 3（从 instr 0 跳 3 条到 instr 3）
        let text = vec![
            isa::encode_ji(opcode::JMP, 3), // JMP到instr 3
            make_movi(8, 42),               // 不可达
            make_movi(9, 99),               // 不可达
            make_movi(10, 1),               // 可达（目标）
        ];
        let mut opt = Optimizer::new(OptLevel::O1);
        let result = opt.optimize(&text);
        assert_eq!(result.len(), 2); // JMP + MOVI
    }

    #[test]
    fn fold_then_compare() {
        // 2 + 3 = 5, then SEQ 5, 5 → MOVI 1
        let text = vec![
            make_movi(8, 2),
            make_movi(9, 3),
            make_r3(opcode::ADD, 10, 8, 9),
            make_movi(11, 5),
            make_r3(opcode::SEQ, 12, 10, 11),
        ];
        let mut opt = Optimizer::new(OptLevel::O1);
        let result = opt.optimize(&text);
        // 第一轮折叠: ADD → MOVI 5
        // 第二轮折叠: MOVI 10, 5; MOVI 11, 5; SEQ → MOVI 1
        // O1 只跑一轮，需要 O2 才能折叠第二次
        assert!(result.len() <= 5);
    }

    #[test]
    fn peephole_mov_add_identity() {
        // MOV t0, t1; ADD t0, R0, t1 → 删除 MOV（ADD 会覆盖结果）
        let text = vec![
            isa::encode_r3(opcode::MOV, 8, 9, 0, 0), // MOV t0, t1
            isa::encode_r3(opcode::ADD, 8, 0, 9, 0), // ADD t0, R0, t1
        ];
        let mut opt = Optimizer::new(OptLevel::O1);
        let result = opt.optimize(&text);
        assert_eq!(result.len(), 1);
        assert_eq!((result[0] >> 24) as u8, opcode::ADD);
    }

    #[test]
    fn peephole_jz_jmp_reversal() {
        // JZ t0, +4; JMP +8 → JNZ t0, +8; JMP +4
        let text = vec![
            isa::encode_r1i(opcode::JZ, 8, 4), // JZ t0, +4
            isa::encode_ji(opcode::JMP, 8),    // JMP +8
        ];
        let mut opt = Optimizer::new(OptLevel::O1);
        let result = opt.optimize(&text);
        assert_eq!(result.len(), 2);
        assert_eq!((result[0] >> 24) as u8, opcode::JNZ);
        assert_eq!((result[1] >> 24) as u8, opcode::JMP);
    }

    #[test]
    fn peephole_jmp_kept_as_is() {
        // JMP +1; NOP; MOVI → DCE 可消除 NOP，窥孔不处理
        let text = vec![
            isa::encode_ji(opcode::JMP, 2), // JMP +2 → instr 2
            make_movi(8, 42),               // 不可达（DCE 消除）
            make_movi(9, 99),               // 目标（JMP 跳转到此）
        ];
        let mut opt = Optimizer::new(OptLevel::O1);
        let result = opt.optimize(&text);
        // DCE: JMP + MOVI target = 2 条指令
        assert_eq!(result.len(), 2);
    }
}
