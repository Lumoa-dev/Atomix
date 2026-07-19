# 冷启动预测误差 — Simulation Report
Generated: 2026-07-20T01:25:39.212539

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 3.9/s | 0.00% | 4746ms | 811ms | 58885ms | 6.6 | 14.6% |
| Baseline (Disc+Mul+Hard+1.5x) | 3.9/s | 0.00% | 4746ms | 811ms | 58885ms | 6.6 | 14.6% |
| FullAdaptive | 4.1/s | 0.00% | 4666ms | 930ms | 59350ms | 6.1 | 22.8% |
| FullOpt | 3.9/s | 0.00% | 4786ms | 767ms | 61952ms | 6.6 | 13.8% |
| FullOpt_v0.3 | 3.9/s | 0.00% | 4786ms | 767ms | 61952ms | 6.6 | 13.8% |
| Lin+WGM+AimdH | 4.3/s | 0.00% | 5911ms | 2009ms | 59811ms | 5.2 | 46.3% |
| MinBottleneck | 3.9/s | 0.00% | 4746ms | 811ms | 58885ms | 6.6 | 14.6% |
| Sig+AimdH | 3.9/s | 0.00% | 4718ms | 792ms | 59769ms | 6.6 | 12.8% |
| Sigmoid Only | 3.9/s | 0.00% | 4718ms | 792ms | 59769ms | 6.6 | 12.8% |

## Rankings

### By Throughput
1. **Lin+WGM+AimdH**: 4.3 tasks/s
2. **FullAdaptive**: 4.1 tasks/s
3. **FullOpt**: 3.9 tasks/s
4. **FullOpt_v0.3**: 3.9 tasks/s
5. **Sigmoid Only**: 3.9 tasks/s
6. **Sig+AimdH**: 3.9 tasks/s
7. **Baseline (Disc+Mul+Hard+1.5x)**: 3.9 tasks/s
8. **MinBottleneck**: 3.9 tasks/s
9. **AIMD+Hysteresis**: 3.9 tasks/s

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