# 内存压力 — Simulation Report
Generated: 2026-07-20T01:23:49.353934

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 3.9/s | 0.00% | 19289ms | 12089ms | 84008ms | 5.6 | 75.8% |
| Baseline (Disc+Mul+Hard+1.5x) | 3.9/s | 0.00% | 19289ms | 12089ms | 84008ms | 5.6 | 75.8% |
| FullAdaptive | 3.9/s | 0.00% | 19295ms | 14416ms | 88405ms | 5.5 | 82.3% |
| FullOpt | 4.1/s | 0.00% | 20182ms | 13948ms | 89084ms | 5.6 | 81.3% |
| FullOpt_v0.3 | 4.1/s | 0.00% | 20182ms | 13948ms | 89084ms | 5.6 | 81.3% |
| Lin+WGM+AimdH | 2.9/s | 0.00% | 21914ms | 14631ms | 84151ms | 5.2 | 82.5% |
| MinBottleneck | 3.9/s | 0.00% | 19289ms | 12089ms | 84008ms | 5.6 | 75.8% |
| Sig+AimdH | 3.9/s | 0.00% | 20460ms | 14926ms | 92266ms | 5.9 | 80.4% |
| Sigmoid Only | 3.9/s | 0.00% | 20460ms | 14926ms | 92266ms | 5.9 | 80.4% |

## Rankings

### By Throughput
1. **FullOpt**: 4.1 tasks/s
2. **FullOpt_v0.3**: 4.1 tasks/s
3. **FullAdaptive**: 3.9 tasks/s
4. **Baseline (Disc+Mul+Hard+1.5x)**: 3.9 tasks/s
5. **MinBottleneck**: 3.9 tasks/s
6. **AIMD+Hysteresis**: 3.9 tasks/s
7. **Sigmoid Only**: 3.9 tasks/s
8. **Sig+AimdH**: 3.9 tasks/s
9. **Lin+WGM+AimdH**: 2.9 tasks/s

### By OOM Rate (lower is better)
1. **Baseline (Disc+Mul+Hard+1.5x)**: 0.00%
2. **Sigmoid Only**: 0.00%
3. **MinBottleneck**: 0.00%
4. **AIMD+Hysteresis**: 0.00%
5. **Sig+AimdH**: 0.00%
6. **Lin+WGM+AimdH**: 0.00%
7. **FullOpt**: 0.00%
8. **FullAdaptive**: 0.00%
9. **FullOpt_v0.3**: 0.00%