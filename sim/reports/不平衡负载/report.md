# 不平衡负载 — Simulation Report
Generated: 2026-07-20T01:25:21.232853

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 18.4/s | 0.00% | 2818ms | 265ms | 22495ms | 13.6 | 15.5% |
| Baseline (Disc+Mul+Hard+1.5x) | 18.4/s | 0.00% | 2818ms | 265ms | 22495ms | 13.6 | 15.5% |
| FullAdaptive | 18.5/s | 0.00% | 2899ms | 265ms | 25779ms | 13.2 | 15.4% |
| FullOpt | 18.5/s | 0.00% | 2882ms | 275ms | 23886ms | 13.4 | 16.0% |
| FullOpt_v0.3 | 18.5/s | 0.00% | 2882ms | 275ms | 23886ms | 13.4 | 16.0% |
| Lin+WGM+AimdH | 18.8/s | 0.00% | 3376ms | 281ms | 37764ms | 12.1 | 20.3% |
| MinBottleneck | 18.4/s | 0.00% | 2818ms | 265ms | 22495ms | 13.6 | 15.5% |
| Sig+AimdH | 18.4/s | 0.00% | 2838ms | 269ms | 23262ms | 13.4 | 15.5% |
| Sigmoid Only | 18.4/s | 0.00% | 2838ms | 269ms | 23262ms | 13.4 | 15.5% |

## Rankings

### By Throughput
1. **Lin+WGM+AimdH**: 18.8 tasks/s
2. **FullAdaptive**: 18.5 tasks/s
3. **FullOpt**: 18.5 tasks/s
4. **FullOpt_v0.3**: 18.5 tasks/s
5. **Sigmoid Only**: 18.4 tasks/s
6. **Sig+AimdH**: 18.4 tasks/s
7. **Baseline (Disc+Mul+Hard+1.5x)**: 18.4 tasks/s
8. **MinBottleneck**: 18.4 tasks/s
9. **AIMD+Hysteresis**: 18.4 tasks/s

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