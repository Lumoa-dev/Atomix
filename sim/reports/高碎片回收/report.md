# 高碎片回收 — Simulation Report
Generated: 2026-07-20T01:25:53.693645

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 5.5/s | 0.00% | 10058ms | 7150ms | 57017ms | 4.3 | 68.3% |
| Baseline (Disc+Mul+Hard+1.5x) | 5.5/s | 0.00% | 10058ms | 7150ms | 57017ms | 4.3 | 68.3% |
| FullAdaptive | 6.4/s | 0.00% | 12683ms | 8808ms | 71254ms | 4.1 | 70.8% |
| FullOpt | 5.9/s | 0.00% | 14346ms | 8879ms | 83658ms | 4.2 | 70.3% |
| FullOpt_v0.3 | 5.9/s | 0.00% | 14346ms | 8879ms | 83658ms | 4.2 | 70.3% |
| Lin+WGM+AimdH | 5.9/s | 0.00% | 14593ms | 15097ms | 76384ms | 3.8 | 76.2% |
| MinBottleneck | 5.5/s | 0.00% | 10058ms | 7150ms | 57017ms | 4.3 | 68.3% |
| Sig+AimdH | 5.8/s | 0.00% | 10680ms | 8621ms | 55650ms | 4.3 | 68.1% |
| Sigmoid Only | 5.8/s | 0.00% | 10680ms | 8621ms | 55650ms | 4.3 | 68.1% |

## Rankings

### By Throughput
1. **FullAdaptive**: 6.4 tasks/s
2. **FullOpt**: 5.9 tasks/s
3. **FullOpt_v0.3**: 5.9 tasks/s
4. **Lin+WGM+AimdH**: 5.9 tasks/s
5. **Sigmoid Only**: 5.8 tasks/s
6. **Sig+AimdH**: 5.8 tasks/s
7. **Baseline (Disc+Mul+Hard+1.5x)**: 5.5 tasks/s
8. **MinBottleneck**: 5.5 tasks/s
9. **AIMD+Hysteresis**: 5.5 tasks/s

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