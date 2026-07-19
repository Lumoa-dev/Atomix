# 震荡测试 — Simulation Report
Generated: 2026-07-20T01:24:26.954521

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 16.8/s | 0.00% | 4391ms | 3459ms | 17168ms | 7.4 | 55.1% |
| Baseline (Disc+Mul+Hard+1.5x) | 16.8/s | 0.00% | 4391ms | 3459ms | 17168ms | 7.4 | 55.1% |
| FullAdaptive | 17.7/s | 0.00% | 4290ms | 1595ms | 55957ms | 7.2 | 30.5% |
| FullOpt | 17.0/s | 0.00% | 3434ms | 1644ms | 20946ms | 7.5 | 30.6% |
| FullOpt_v0.3 | 17.0/s | 0.00% | 3434ms | 1644ms | 20946ms | 7.5 | 30.6% |
| Lin+WGM+AimdH | 17.5/s | 0.00% | 4509ms | 3362ms | 23413ms | 5.8 | 62.3% |
| MinBottleneck | 16.8/s | 0.00% | 4391ms | 3459ms | 17168ms | 7.4 | 55.1% |
| Sig+AimdH | 16.9/s | 0.00% | 3422ms | 1684ms | 21770ms | 7.6 | 32.9% |
| Sigmoid Only | 16.9/s | 0.00% | 3422ms | 1684ms | 21770ms | 7.6 | 32.9% |

## Rankings

### By Throughput
1. **FullAdaptive**: 17.7 tasks/s
2. **Lin+WGM+AimdH**: 17.5 tasks/s
3. **FullOpt**: 17.0 tasks/s
4. **FullOpt_v0.3**: 17.0 tasks/s
5. **Sigmoid Only**: 16.9 tasks/s
6. **Sig+AimdH**: 16.9 tasks/s
7. **Baseline (Disc+Mul+Hard+1.5x)**: 16.8 tasks/s
8. **MinBottleneck**: 16.8 tasks/s
9. **AIMD+Hysteresis**: 16.8 tasks/s

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