#!/usr/bin/env python3
"""
Atomix 运行时策略仿真 — 主入口
===============================
运行所有场景 × 所有算法变体，生成对比报告。

用法:
    python -m sim.main              # 运行所有场景（可能较慢）
    python -m sim.main --quick      # 快速模式（仅2个场景）
    python -m sim.main --scenario steady  # 仅运行指定场景
"""

import sys
import os
import argparse
from datetime import datetime

# 确保可以导入 sim 包
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from sim.config import (
    HardwareConfig, AlgorithmConfig, ArrivalConfig, SimulationConfig,
    SmoothingMethod, MergeStrategy, OOMFeedback, SlipwayStrategy
)
from sim.scenarios import (
    ALL_SCENARIOS, SCENARIO_STEADY, SCENARIO_BURST,
    SCENARIO_MEM_PRESSURE, SCENARIO_CPU_PRESSURE,
    SCENARIO_OSCILLATION, SCENARIO_MARATHON,
    get_algorithm_variants
)
from sim.adaptive_controller import AdaptiveResourceController
from sim.simulation import run_scenario
from sim.report_generator import ReportGenerator


def parse_args():
    parser = argparse.ArgumentParser(description="Atomix Runtime Strategy Simulation")
    parser.add_argument("--quick", action="store_true",
                        help="Quick mode: only run 2 scenarios with fewer variants")
    parser.add_argument("--scenario", type=str, default=None,
                        choices=["steady", "burst", "mem", "cpu", "oscillation", "marathon", "all"],
                        help="Run specific scenario only")
    parser.add_argument("--duration", type=float, default=None,
                        help="Override simulation duration (seconds)")
    parser.add_argument("--variants", type=str, default=None,
                        help="Comma-separated variant names to test (or 'all')")
    return parser.parse_args()


def main():
    args = parse_args()

    print("=" * 60)
    print("Atomix Runtime Strategy — Simulation Framework")
    print(f"Started: {datetime.now().isoformat()}")
    print("=" * 60)

    # 确定要跑的场景
    if args.scenario and args.scenario != "all":
        scenario_map = {
            "steady": SCENARIO_STEADY,
            "burst": SCENARIO_BURST,
            "mem": SCENARIO_MEM_PRESSURE,
            "cpu": SCENARIO_CPU_PRESSURE,
            "oscillation": SCENARIO_OSCILLATION,
            "marathon": SCENARIO_MARATHON,
        }
        scenarios = [scenario_map[args.scenario]]
    elif args.quick:
        # 快速模式：只用稳态和突发，缩短时长
        s1 = dict(SCENARIO_STEADY)
        s1["simulation"] = SimulationConfig(duration_sec=30.0, warmup_sec=5.0, seed=42)
        s2 = dict(SCENARIO_BURST)
        s2["simulation"] = SimulationConfig(duration_sec=30.0, warmup_sec=5.0, seed=123)
        scenarios = [s1, s2]
        print("Quick mode: 2 scenarios, 30s each")
    else:
        scenarios = ALL_SCENARIOS

    if args.duration:
        for s in scenarios:
            s["simulation"] = SimulationConfig(
                duration_sec=args.duration,
                warmup_sec=min(10.0, args.duration * 0.1),
                seed=s["simulation"].seed
            )

    # 获取算法变体
    hw = scenarios[0]["hardware"]
    all_variants = get_algorithm_variants(hw)

    if args.quick:
        # 快速模式：只用 4 个代表性变体
        wanted = ["Baseline (Disc+Mul+Hard+1.5x)", "Sigmoid Only",
                   "AIMD+Hysteresis", "FullOpt"]
        variants = [(name, ctrl) for name, ctrl in all_variants if name in wanted]
    elif args.variants and args.variants != "all":
        wanted = set(args.variants.split(","))
        variants = [(name, ctrl) for name, ctrl in all_variants if name in wanted]
    else:
        variants = all_variants

    print(f"\nScenarios: {len(scenarios)}")
    print(f"Algorithm variants: {len(variants)}")
    for name, _ in variants:
        print(f"  - {name}")

    # 跑所有场景
    all_results = {}
    report_gen = ReportGenerator("sim/reports")

    for scenario_dict in scenarios:
        sname = scenario_dict["name"]
        hw_cfg = scenario_dict["hardware"]
        profiles = scenario_dict["profiles"]
        arrival = scenario_dict["arrival"]
        sim_cfg = scenario_dict["simulation"]

        results = run_scenario(
            sname, hw_cfg, variants, profiles, arrival, sim_cfg
        )
        all_results[sname] = results

        # 生成该场景的报告
        report_gen.generate_scenario_report(sname, results)

    # 生成汇总 Dashboard
    if len(all_results) > 1:
        report_gen.generate_dashboard(all_results)

    # 打印最终汇总
    print("\n" + "=" * 60)
    print("FINAL SUMMARY")
    print("=" * 60)

    for sname, results in all_results.items():
        print(f"\n## {sname}")
        print(f"{'Algorithm':<35} {'Throughput':>10} {'OOM%':>8} {'AvgLat':>8} {'P99':>8} {'N_batch':>8}")
        print("-" * 80)
        for label, metrics in sorted(results.items()):
            s = metrics.summary()
            print(f"{label:<35} {s.get('throughput_per_sec', 0):>8.1f}/s "
                  f"{s.get('oom_rate', 0)*100:>7.2f}% "
                  f"{s.get('avg_latency_ms', 0):>7.0f}ms "
                  f"{s.get('p99_latency_ms', 0):>7.0f}ms "
                  f"{s.get('avg_n_batch', 0):>7.1f}")

    print(f"\nReports saved to: sim/reports/")
    print(f"Done at: {datetime.now().isoformat()}")


if __name__ == "__main__":
    main()
