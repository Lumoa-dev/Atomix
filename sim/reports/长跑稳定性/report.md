# 长跑稳定性 — Simulation Report
Generated: 2026-07-20T01:25:03.991793

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 10.6/s | 0.00% | 4738ms | 2532ms | 53682ms | 8.7 | 38.8% |
| Baseline (Disc+Mul+Hard+1.5x) | 10.6/s | 0.00% | 4738ms | 2532ms | 53682ms | 8.7 | 38.8% |
| FullAdaptive | 10.9/s | 0.00% | 3208ms | 1442ms | 28977ms | 8.3 | 19.9% |
| FullOpt | 10.7/s | 0.00% | 3730ms | 1599ms | 39952ms | 8.6 | 19.8% |
| FullOpt_v0.3 | 10.7/s | 0.00% | 3730ms | 1599ms | 39952ms | 8.6 | 19.8% |
| Lin+WGM+AimdH | 11.3/s | 0.00% | 4710ms | 2712ms | 31205ms | 7.1 | 56.4% |
| MinBottleneck | 10.6/s | 0.00% | 4738ms | 2532ms | 53682ms | 8.7 | 38.8% |
| Sig+AimdH | 10.7/s | 0.00% | 3562ms | 1534ms | 29506ms | 8.6 | 17.4% |
| Sigmoid Only | 10.7/s | 0.00% | 3562ms | 1534ms | 29506ms | 8.6 | 17.4% |

## Rankings

### By Throughput
1. **Lin+WGM+AimdH**: 11.3 tasks/s
2. **FullAdaptive**: 10.9 tasks/s
3. **FullOpt**: 10.7 tasks/s
4. **FullOpt_v0.3**: 10.7 tasks/s
5. **Sigmoid Only**: 10.7 tasks/s
6. **Sig+AimdH**: 10.7 tasks/s
7. **Baseline (Disc+Mul+Hard+1.5x)**: 10.6 tasks/s
8. **MinBottleneck**: 10.6 tasks/s
9. **AIMD+Hysteresis**: 10.6 tasks/s

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