# CPU压力 — Simulation Report
Generated: 2026-07-20T01:24:07.693523

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 27.8/s | 0.00% | 1749ms | 317ms | 8424ms | 19.9 | 10.8% |
| Baseline (Disc+Mul+Hard+1.5x) | 27.8/s | 0.00% | 1749ms | 317ms | 8424ms | 19.9 | 10.8% |
| FullAdaptive | 27.8/s | 0.00% | 1658ms | 310ms | 8130ms | 20.0 | 9.9% |
| FullOpt | 27.7/s | 0.00% | 1666ms | 309ms | 8113ms | 19.9 | 8.0% |
| FullOpt_v0.3 | 27.7/s | 0.00% | 1666ms | 309ms | 8113ms | 19.9 | 8.0% |
| Lin+WGM+AimdH | 28.5/s | 0.00% | 2104ms | 279ms | 17133ms | 18.1 | 10.0% |
| MinBottleneck | 27.8/s | 0.00% | 1749ms | 317ms | 8424ms | 19.9 | 10.8% |
| Sig+AimdH | 27.7/s | 0.00% | 1704ms | 312ms | 8186ms | 20.0 | 8.8% |
| Sigmoid Only | 27.7/s | 0.00% | 1704ms | 312ms | 8186ms | 20.0 | 8.8% |

## Rankings

### By Throughput
1. **Lin+WGM+AimdH**: 28.5 tasks/s
2. **Baseline (Disc+Mul+Hard+1.5x)**: 27.8 tasks/s
3. **MinBottleneck**: 27.8 tasks/s
4. **AIMD+Hysteresis**: 27.8 tasks/s
5. **FullAdaptive**: 27.8 tasks/s
6. **Sigmoid Only**: 27.7 tasks/s
7. **Sig+AimdH**: 27.7 tasks/s
8. **FullOpt**: 27.7 tasks/s
9. **FullOpt_v0.3**: 27.7 tasks/s

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