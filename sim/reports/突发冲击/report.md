# 突发冲击 — Simulation Report
Generated: 2026-07-20T01:23:36.105722

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 14.0/s | 0.00% | 2208ms | 1087ms | 14097ms | 11.3 | 39.0% |
| Baseline (Disc+Mul+Hard+1.5x) | 14.0/s | 0.00% | 2208ms | 1087ms | 14097ms | 11.3 | 39.0% |
| FullAdaptive | 14.3/s | 0.00% | 2022ms | 590ms | 14424ms | 11.3 | 15.0% |
| FullOpt | 14.1/s | 0.00% | 2029ms | 586ms | 14709ms | 11.4 | 10.6% |
| FullOpt_v0.3 | 14.1/s | 0.00% | 2029ms | 586ms | 14709ms | 11.4 | 10.6% |
| Lin+WGM+AimdH | 15.2/s | 0.00% | 3209ms | 1083ms | 30342ms | 9.8 | 43.2% |
| MinBottleneck | 14.0/s | 0.00% | 2208ms | 1087ms | 14097ms | 11.3 | 39.0% |
| Sig+AimdH | 14.0/s | 0.00% | 2045ms | 593ms | 14332ms | 11.4 | 11.3% |
| Sigmoid Only | 14.0/s | 0.00% | 2045ms | 593ms | 14332ms | 11.4 | 11.3% |

## Rankings

### By Throughput
1. **Lin+WGM+AimdH**: 15.2 tasks/s
2. **FullAdaptive**: 14.3 tasks/s
3. **FullOpt**: 14.1 tasks/s
4. **FullOpt_v0.3**: 14.1 tasks/s
5. **Sigmoid Only**: 14.0 tasks/s
6. **Sig+AimdH**: 14.0 tasks/s
7. **Baseline (Disc+Mul+Hard+1.5x)**: 14.0 tasks/s
8. **MinBottleneck**: 14.0 tasks/s
9. **AIMD+Hysteresis**: 14.0 tasks/s

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