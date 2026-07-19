# 大批量小任务预载 — Simulation Report
Generated: 2026-07-20T01:26:09.500726

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 47.5 | 0.8% |
| Baseline (Disc+Mul+Hard+1.5x) | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 47.5 | 0.8% |
| FullAdaptive | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 45.6 | 0.7% |
| FullOpt | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 45.6 | 0.7% |
| FullOpt_v0.3 | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 45.6 | 0.7% |
| Lin+WGM+AimdH | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 45.9 | 0.7% |
| MinBottleneck | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 47.5 | 0.8% |
| Sig+AimdH | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 45.6 | 0.7% |
| Sigmoid Only | 51.1/s | 0.00% | 36ms | 33ms | 77ms | 45.6 | 0.7% |

## Rankings

### By Throughput
1. **Baseline (Disc+Mul+Hard+1.5x)**: 51.1 tasks/s
2. **Sigmoid Only**: 51.1 tasks/s
3. **MinBottleneck**: 51.1 tasks/s
4. **AIMD+Hysteresis**: 51.1 tasks/s
5. **Sig+AimdH**: 51.1 tasks/s
6. **Lin+WGM+AimdH**: 51.1 tasks/s
7. **FullOpt**: 51.1 tasks/s
8. **FullAdaptive**: 51.1 tasks/s
9. **FullOpt_v0.3**: 51.1 tasks/s

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