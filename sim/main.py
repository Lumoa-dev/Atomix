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
import json
import argparse
from datetime import datetime
from dataclasses import replace

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
    SCENARIO_UNBALANCED, SCENARIO_COLD_START_ERROR,
    SCENARIO_HIGH_FRAGMENTATION, SCENARIO_BATCH_PREFETCH,
    get_algorithm_variants
)
from sim.adaptive_controller import AdaptiveResourceController
from sim.simulation import run_scenario, run_single_simulation
from sim.report_generator import ReportGenerator


def parse_args():
    parser = argparse.ArgumentParser(description="Atomix Runtime Strategy Simulation")
    parser.add_argument("--quick", action="store_true",
                        help="Quick mode: only run 2 scenarios with fewer variants")
    parser.add_argument("--scenario", type=str, default=None,
                        choices=["steady", "burst", "mem", "cpu", "oscillation",
                                 "marathon", "unbalanced", "cold_start", "frag", "prefetch", "all"],
                        help="Run specific scenario only")
    parser.add_argument("--duration", type=float, default=None,
                        help="Override simulation duration (seconds)")
    parser.add_argument("--variants", type=str, default=None,
                        help="Comma-separated variant names to test (or 'all')")
    parser.add_argument("--scan", action="store_true",
                        help="Full-parameter scan mode")
    parser.add_argument("--scan-params", type=str, default=None,
                        help="Parameters to scan (json or comma-separated)")
    parser.add_argument("--export-csv", action="store_true", default=True,
                        help="Export raw CSV data")
    parser.add_argument("--detailed", action="store_true",
                        help="Generate detailed charts (time series, factor decomposition, etc.)")
    return parser.parse_args()


def run_scan(hw_config: HardwareConfig, algo_config: AlgorithmConfig,
             scenarios: list):
    """Full-parameter scan over CPU/memory/arrival rate combinations."""
    from itertools import product
    import csv

    scan_params = {
        "cpu_cores": [4, 8, 16],
        "mem_free_mb": [1024, 2048, 4096, 8192],
        "arrival_rate": [2, 5, 10, 20, 50],
    }

    results = []
    first_scenario = scenarios[0]
    csv_dir = getattr(first_scenario["simulation"], 'csv_dir', 'sim/reports/csv')

    for cpu, mem, rate in product(*scan_params.values()):
        hw = replace(hw_config, cpu_cores=cpu, mem_free_mb=mem)
        arrival = replace(first_scenario["arrival"], rate_per_sec=rate)
        sim_cfg = replace(first_scenario["simulation"],
                         duration_sec=60.0, warmup_sec=5.0)

        # Run a quick simulation
        _, metrics = run_single_simulation(
            hw, algo_config,
            first_scenario["profiles"], arrival, sim_cfg
        )
        s = metrics.summary()
        results.append({
            "cpu": cpu, "mem_mb": mem, "arrival_rate": rate,
            "throughput": s.get("throughput_per_sec", 0),
            "oom_rate": s.get("oom_rate", 0),
            "avg_latency_ms": s.get("avg_latency_ms", 0),
            "avg_n_batch": s.get("avg_n_batch", 0),
            "avg_utilization": s.get("avg_utilization", 0),
        })

        # Export CSV for each run
        if sim_cfg.export_csv:
            csv_path = f"{csv_dir}/scan_c{cpu}_m{mem}_r{rate}.csv"
            metrics.export_csv(csv_path)

    # Export scan results as CSV
    os.makedirs(csv_dir, exist_ok=True)
    scan_summary_path = f"{csv_dir}/scan_summary.csv"
    with open(scan_summary_path, 'w', newline='') as f:
        if results:
            w = csv.DictWriter(f, fieldnames=results[0].keys())
            w.writeheader()
            w.writerows(results)

    print(f"\nScan results saved to: {scan_summary_path}")
    print(f"Total scan combinations: {len(results)}")
    return results


def main():
    args = parse_args()

    print("=" * 60)
    print("Atomix Runtime Strategy — Simulation Framework")
    print(f"Started: {datetime.now().isoformat()}")
    print("=" * 60)

    # ── 全参数扫描模式 ──
    if args.scan:
        print("\n[Scan Mode] Full-parameter scan enabled")
        hw = HardwareConfig()
        algo_cfg = AlgorithmConfig()
        results = run_scan(hw, algo_cfg, ALL_SCENARIOS)
        print(f"\nDone at: {datetime.now().isoformat()}")
        return

    # 确定要跑的场景
    if args.scenario and args.scenario != "all":
        scenario_map = {
            "steady": SCENARIO_STEADY,
            "burst": SCENARIO_BURST,
            "mem": SCENARIO_MEM_PRESSURE,
            "cpu": SCENARIO_CPU_PRESSURE,
            "oscillation": SCENARIO_OSCILLATION,
            "marathon": SCENARIO_MARATHON,
            "unbalanced": SCENARIO_UNBALANCED,
            "cold_start": SCENARIO_COLD_START_ERROR,
            "frag": SCENARIO_HIGH_FRAGMENTATION,
            "prefetch": SCENARIO_BATCH_PREFETCH,
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
    report_gen = ReportGenerator("sim/reports", detailed=args.detailed)

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

        # 导出 CSV 原始数据到场景目录
        if sim_cfg.export_csv:
            csv_dir = f"{sim_cfg.report_dir}/{sname}/csv"
            import os
            os.makedirs(csv_dir, exist_ok=True)
            for label, metrics in results.items():
                safe_label = label.replace(" ", "_").replace("(", "").replace(")", "").replace("+", "p")
                csv_path = f"{csv_dir}/{safe_label}.csv"
                metrics.export_csv(csv_path)
            print(f"  CSV data exported to: {csv_dir}")

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
