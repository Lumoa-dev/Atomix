# 稳态混合负载 — Simulation Report
Generated: 2026-07-20T01:23:24.433381

## Summary Table

| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |
|-----------|-----------|----------|---------|-----|-----|-------------|------|
| AIMD+Hysteresis | 12.4/s | 0.00% | 2971ms | 2172ms | 16532ms | 10.6 | 43.4% |
| Baseline (Disc+Mul+Hard+1.5x) | 12.4/s | 0.00% | 2971ms | 2172ms | 16532ms | 10.6 | 43.4% |
| FullAdaptive | 13.0/s | 0.00% | 2769ms | 823ms | 29210ms | 10.5 | 17.8% |
| FullOpt | 12.7/s | 0.00% | 2391ms | 826ms | 19468ms | 10.6 | 14.3% |
| FullOpt_v0.3 | 12.7/s | 0.00% | 2391ms | 826ms | 19468ms | 10.6 | 14.3% |
| Lin+WGM+AimdH | 13.6/s | 0.00% | 3190ms | 1184ms | 30041ms | 8.9 | 35.8% |
| MinBottleneck | 12.4/s | 0.00% | 2971ms | 2172ms | 16532ms | 10.6 | 43.4% |
| Sig+AimdH | 12.6/s | 0.00% | 2375ms | 856ms | 18509ms | 10.7 | 14.8% |
| Sigmoid Only | 12.6/s | 0.00% | 2375ms | 856ms | 18509ms | 10.7 | 14.8% |

## Rankings

### By Throughput
1. **Lin+WGM+AimdH**: 13.6 tasks/s
2. **FullAdaptive**: 13.0 tasks/s
3. **FullOpt**: 12.7 tasks/s
4. **FullOpt_v0.3**: 12.7 tasks/s
5. **Sigmoid Only**: 12.6 tasks/s
6. **Sig+AimdH**: 12.6 tasks/s
7. **Baseline (Disc+Mul+Hard+1.5x)**: 12.4 tasks/s
8. **MinBottleneck**: 12.4 tasks/s
9. **AIMD+Hysteresis**: 12.4 tasks/s

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