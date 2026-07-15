"""
报告生成器
==========
汇总仿真结果，生成文本报告和调用可视化。
"""

import os
import json
from typing import Dict, List
from datetime import datetime

from sim.metrics import MetricsCollector
from sim.visualizer import (
    setup_style, plot_throughput_comparison, plot_time_series,
    plot_factor_decomposition, plot_heatmap, plot_pareto_frontier,
    plot_dashboard
)


class ReportGenerator:
    """报告生成器"""

    def __init__(self, output_dir: str = "sim/reports"):
        self.output_dir = output_dir
        os.makedirs(output_dir, exist_ok=True)
        setup_style()

    def generate_scenario_report(self, scenario_name: str,
                                  results: Dict[str, MetricsCollector],
                                  algo_configs: dict = None):
        """为一个场景生成完整报告"""
        safe_name = scenario_name.replace(" ", "_").replace("/", "_")
        scenario_dir = os.path.join(self.output_dir, safe_name)
        os.makedirs(scenario_dir, exist_ok=True)

        print(f"\nGenerating report for: {scenario_name}")

        # ── 1. 吞吐量对比图 ──
        plot_throughput_comparison(
            results,
            f"{scenario_name} — Algorithm Comparison",
            os.path.join(scenario_dir, "throughput_comparison.png")
        )

        # ── 2. 时间序列图 ──
        plot_time_series(
            results,
            f"{scenario_name} — Time Series",
            os.path.join(scenario_dir, "time_series.png")
        )

        # ── 3. 热力图 ──
        plot_heatmap(
            results,
            f"{scenario_name} — Metrics Heatmap",
            os.path.join(scenario_dir, "heatmap.png")
        )

        # ── 4. 帕累托前沿 ──
        plot_pareto_frontier(
            results,
            scenario_name,
            os.path.join(scenario_dir, "pareto_frontier.png")
        )

        # ── 5. 因子分解图（只对最佳算法） ──
        best_label = self._find_best(results)
        if best_label and best_label in results:
            plot_factor_decomposition(
                results[best_label],
                best_label,
                os.path.join(scenario_dir, "factor_decomposition.png")
            )

        # ── 6. JSON 汇总 ──
        summary_data = {}
        for label, metrics in results.items():
            summary_data[label] = metrics.summary()

        with open(os.path.join(scenario_dir, "summary.json"), "w") as f:
            json.dump(summary_data, f, indent=2, default=str)

        # ── 7. 文本报告 ──
        self._write_text_report(scenario_name, results, scenario_dir)

        print(f"  Report saved to: {scenario_dir}")

    def generate_dashboard(self, all_results: Dict[str, Dict[str, MetricsCollector]]):
        """生成跨场景汇总 Dashboard"""
        dashboard_path = os.path.join(self.output_dir, "dashboard.png")
        plot_dashboard(all_results, dashboard_path)

        # 汇总 JSON
        master_summary = {}
        for sname, results in all_results.items():
            master_summary[sname] = {}
            for label, metrics in results.items():
                master_summary[sname][label] = metrics.summary()

        with open(os.path.join(self.output_dir, "master_summary.json"), "w") as f:
            json.dump(master_summary, f, indent=2, default=str)

    def _find_best(self, results: Dict[str, MetricsCollector]) -> str:
        """找最佳算法（综合评分：吞吐量高 + OOM 率低 + 延迟低）"""
        best_score = -1
        best_label = None

        for label, metrics in results.items():
            s = metrics.summary()
            tp = s.get("throughput_per_sec", 0)
            oom = s.get("oom_rate", 0)
            lat = s.get("avg_latency_ms", 99999)

            # 综合评分：吞吐量 - OOM惩罚 - 延迟惩罚
            score = tp * (1.0 - min(oom * 10, 0.9)) - lat * 0.01
            if score > best_score:
                best_score = score
                best_label = label

        return best_label

    def _write_text_report(self, name: str, results: Dict[str, MetricsCollector], out_dir: str):
        """写文本报告"""
        lines = []
        lines.append(f"# {name} — Simulation Report")
        lines.append(f"Generated: {datetime.now().isoformat()}")
        lines.append("")

        lines.append("## Summary Table")
        lines.append("")
        lines.append("| Algorithm | Throughput | OOM Rate | Avg Lat | P50 | P99 | Avg N_batch | Util |")
        lines.append("|-----------|-----------|----------|---------|-----|-----|-------------|------|")

        for label, metrics in sorted(results.items()):
            s = metrics.summary()
            lines.append(
                f"| {label} | {s.get('throughput_per_sec', 0):.1f}/s | "
                f"{s.get('oom_rate', 0)*100:.2f}% | "
                f"{s.get('avg_latency_ms', 0):.0f}ms | "
                f"{s.get('p50_latency_ms', 0):.0f}ms | "
                f"{s.get('p99_latency_ms', 0):.0f}ms | "
                f"{s.get('avg_n_batch', 0):.1f} | "
                f"{s.get('avg_utilization', 0)*100:.1f}% |"
            )

        lines.append("")
        lines.append("## Rankings")
        lines.append("")

        # 按吞吐量排名
        lines.append("### By Throughput")
        sorted_by_tp = sorted(results.items(),
                              key=lambda x: x[1].summary().get("throughput_per_sec", 0),
                              reverse=True)
        for i, (label, m) in enumerate(sorted_by_tp):
            lines.append(f"{i+1}. **{label}**: {m.summary().get('throughput_per_sec', 0):.1f} tasks/s")

        # 按 OOM 率排名
        lines.append("")
        lines.append("### By OOM Rate (lower is better)")
        sorted_by_oom = sorted(results.items(),
                               key=lambda x: x[1].summary().get("oom_rate", 999))
        for i, (label, m) in enumerate(sorted_by_oom):
            lines.append(f"{i+1}. **{label}**: {m.summary().get('oom_rate', 0)*100:.2f}%")

        report_path = os.path.join(out_dir, "report.md")
        with open(report_path, "w", encoding="utf-8") as f:
            f.write("\n".join(lines))

        print(f"  Saved: {report_path}")
