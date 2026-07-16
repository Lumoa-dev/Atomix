"""
Run full simulation suite and generate reports.
Usage: python run_all.py
"""

import warnings
warnings.filterwarnings('ignore')

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
plt.rcParams['font.family'] = 'sans-serif'

import json
import os
import time
from datetime import datetime

from sim.scenarios import (
    ALL_SCENARIOS, SCENARIO_STEADY, SCENARIO_BURST,
    SCENARIO_MEM_PRESSURE, SCENARIO_CPU_PRESSURE,
    SCENARIO_OSCILLATION, SCENARIO_MARATHON,
    get_algorithm_variants
)
from sim.simulation import run_scenario
from sim.report_generator import ReportGenerator
from sim.visualizer import (
    setup_style, plot_throughput_comparison, plot_time_series,
    plot_factor_decomposition, plot_heatmap, plot_pareto_frontier,
    plot_dashboard
)


def main():
    t0 = time.time()
    print("=" * 60)
    print("Atomix Runtime Strategy — Full Simulation Suite")
    print(f"Started: {datetime.now().isoformat()}")
    print("=" * 60)

    scenarios = ALL_SCENARIOS
    all_results = {}
    report_gen = ReportGenerator("sim/reports")

    # Get algorithm variants
    hw = scenarios[0]["hardware"]
    all_variants = get_algorithm_variants(hw)
    print(f"\nScenarios: {len(scenarios)}")
    print(f"Algorithm variants: {len(all_variants)}")
    for name, _ in all_variants:
        print(f"  - {name}")

    for scenario_dict in scenarios:
        sname = scenario_dict["name"]
        hw_cfg = scenario_dict["hardware"]
        profiles = scenario_dict["profiles"]
        arrival = scenario_dict["arrival"]
        sim_cfg = scenario_dict["simulation"]

        print(f"\n{'='*60}")
        print(f"Running: {sname} ({sim_cfg.duration_sec}s)")
        print(f"{'='*60}")

        results = run_scenario(sname, hw_cfg, all_variants, profiles, arrival, sim_cfg)
        all_results[sname] = results

        # Generate report (with font fix — use ASCII-safe labels)
        safe_name = sname.replace(" ", "_").replace("/", "_")
        scenario_dir = os.path.join("sim/reports", safe_name)
        os.makedirs(scenario_dir, exist_ok=True)

        try:
            plot_throughput_comparison(results, sname, os.path.join(scenario_dir, "throughput.png"))
            plot_time_series(results, sname, os.path.join(scenario_dir, "timeseries.png"))
            plot_heatmap(results, sname, os.path.join(scenario_dir, "heatmap.png"))
            plot_pareto_frontier(results, sname, os.path.join(scenario_dir, "pareto.png"))
        except Exception as e:
            print(f"  Chart warning (non-fatal): {e}")

        # Save JSON
        summary_data = {}
        for label, metrics in results.items():
            summary_data[label] = metrics.summary()
        with open(os.path.join(scenario_dir, "summary.json"), "w") as f:
            json.dump(summary_data, f, indent=2, default=str)

    # Dashboard
    if len(all_results) > 1:
        try:
            plot_dashboard(all_results, "sim/reports/dashboard.png")
        except Exception as e:
            print(f"  Dashboard warning: {e}")

    # Master summary
    master = {}
    for sname, results in all_results.items():
        master[sname] = {}
        for label, metrics in results.items():
            master[sname][label] = metrics.summary()
    with open("sim/reports/master_summary.json", "w") as f:
        json.dump(master, f, indent=2, default=str)

    # Print final summary
    print("\n" + "=" * 60)
    print("FINAL SUMMARY")
    print(f"Total time: {time.time() - t0:.1f}s")
    print("=" * 60)

    for sname, results in all_results.items():
        print(f"\n## {sname}")
        print(f"{'Algorithm':<35} {'TP':>6} {'OOM%':>7} {'AvgLat':>7} {'P99':>7} {'N_bat':>6} {'Util%':>6}")
        print("-" * 85)
        for label, metrics in sorted(results.items()):
            s = metrics.summary()
            print(f"{label:<35} {s['throughput_per_sec']:>5.1f}/s "
                  f"{s['oom_rate']*100:>6.2f}% "
                  f"{s['avg_latency_ms']:>6.0f}ms "
                  f"{s['p99_latency_ms']:>6.0f}ms "
                  f"{s['avg_n_batch']:>5.1f} "
                  f"{s['avg_utilization']*100:>5.1f}%")

    print(f"\nReports: sim/reports/")
    print(f"Done: {datetime.now().isoformat()}")


if __name__ == "__main__":
    main()
