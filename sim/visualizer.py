"""
可视化模块
==========
使用 matplotlib + seaborn 生成仿真结果图表。
"""

import os
from typing import Dict, List
import numpy as np

from sim.metrics import MetricsCollector


def setup_style():
    """设置绘图样式（中英文均正确渲染）"""
    try:
        import matplotlib
        matplotlib.use('Agg')
        import matplotlib.pyplot as plt
        import seaborn as sns

        # 先设 seaborn 风格（会冲掉部分 rcParams，所以字体要放后面设）
        sns.set_style("whitegrid")

        # ── 中文字体：直接设 font.sans-serif 候选列表，不依赖 findfont ──
        # Windows 典型中文字体，按优先级排列
        zh_fonts = [
            'Microsoft YaHei',      # 微软雅黑
            'SimHei',               # 黑体
            'Noto Sans SC',         # Google Noto
            'DengXian',             # 等线
            'Microsoft JhengHei',   # 微软正黑
            'DejaVu Sans',
        ]
        plt.rcParams['font.sans-serif'] = zh_fonts
        plt.rcParams['axes.unicode_minus'] = False

        # ── 全局样式 ──
        plt.rcParams.update({
            'figure.dpi': 150,
            'savefig.dpi': 150,
            'font.size': 10,
            'axes.titlesize': 13,
            'axes.labelsize': 11,
            'figure.facecolor': 'white',
            'savefig.facecolor': 'white',
            'savefig.bbox': 'tight',
        })
    except ImportError:
        pass


def plot_throughput_comparison(results: Dict[str, MetricsCollector],
                                title: str, save_path: str):
    """吞吐量对比图"""
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib not installed, skipping plot")
        return

    labels = list(results.keys())
    throughputs = [r.summary().get("throughput_per_sec", 0) for r in results.values()]
    oom_rates = [r.summary().get("oom_rate", 0) * 100 for r in results.values()]
    avg_latencies = [r.summary().get("avg_latency_ms", 0) for r in results.values()]

    fig, axes = plt.subplots(1, 3, figsize=(18, 5))

    colors = plt.cm.viridis(np.linspace(0.2, 0.9, len(labels)))

    # 吞吐量
    bars = axes[0].bar(range(len(labels)), throughputs, color=colors)
    axes[0].set_xticks(range(len(labels)))
    axes[0].set_xticklabels(labels, rotation=45, ha='right', fontsize=8)
    axes[0].set_ylabel("Throughput (tasks/s)")
    axes[0].set_title("Throughput")
    for bar, val in zip(bars, throughputs):
        axes[0].text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.1,
                     f'{val:.1f}', ha='center', va='bottom', fontsize=7)

    # OOM 率
    bars = axes[1].bar(range(len(labels)), oom_rates, color=colors)
    axes[1].set_xticks(range(len(labels)))
    axes[1].set_xticklabels(labels, rotation=45, ha='right', fontsize=8)
    axes[1].set_ylabel("OOM Rate (%)")
    axes[1].set_title("OOM Rate (lower is better)")
    axes[1].axhline(y=2.0, color='red', linestyle='--', alpha=0.5, label='2% threshold')
    axes[1].legend(fontsize=7)
    for bar, val in zip(bars, oom_rates):
        axes[1].text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.05,
                     f'{val:.2f}%', ha='center', va='bottom', fontsize=7)

    # 平均延迟
    bars = axes[2].bar(range(len(labels)), avg_latencies, color=colors)
    axes[2].set_xticks(range(len(labels)))
    axes[2].set_xticklabels(labels, rotation=45, ha='right', fontsize=8)
    axes[2].set_ylabel("Avg Latency (ms)")
    axes[2].set_title("Average Task Latency")

    fig.suptitle(title, fontsize=14, fontweight='bold')
    plt.tight_layout()
    plt.savefig(save_path, bbox_inches='tight')
    plt.close()
    print(f"  Saved: {save_path}")


def plot_time_series(results: Dict[str, MetricsCollector],
                     title: str, save_path: str):
    """时间序列对比图（多算法在一张图上）"""
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib not installed")
        return

    fig, axes = plt.subplots(3, 2, figsize=(16, 12))
    colors = plt.cm.tab10(np.linspace(0, 1, len(results)))

    for idx, (label, metrics) in enumerate(results.items()):
        arr = metrics.get_arrays()
        if not arr:
            continue
        color = colors[idx]
        time = arr["time"]

        # N_batch
        axes[0, 0].plot(time, arr["n_batch"], color=color, alpha=0.8, linewidth=1.2, label=label)
        axes[0, 0].set_ylabel("N_batch")
        axes[0, 0].set_title("Batch Size over Time")
        axes[0, 0].legend(fontsize=6, loc='upper right')

        # Pool depth
        axes[0, 1].plot(time, arr["pool_depth"], color=color, alpha=0.8, linewidth=1.2)
        axes[0, 1].set_ylabel("Pool Depth")
        axes[0, 1].set_title("Task Pool Backlog")

        # Cumulative completed
        axes[1, 0].plot(time, arr["tasks_completed_cum"], color=color, alpha=0.8, linewidth=1.2)
        axes[1, 0].set_ylabel("Completed")
        axes[1, 0].set_title("Cumulative Completed Tasks")

        # Merged factor
        axes[1, 1].plot(time, arr["merged_factor"], color=color, alpha=0.8, linewidth=1.2)
        axes[1, 1].set_ylabel("Merged Factor")
        axes[1, 1].set_title("Merged Adjustment Factor")
        axes[1, 1].axhline(y=1.0, color='gray', linestyle='--', alpha=0.3)

        # Memory utilization
        axes[2, 0].plot(time, arr["mem_util"] * 100, color=color, alpha=0.8, linewidth=1.2)
        axes[2, 0].set_ylabel("Memory Util (%)")
        axes[2, 0].set_title("Memory Utilization")
        axes[2, 0].set_xlabel("Time (s)")

        # Latency
        axes[2, 1].plot(time, arr["p50_latency_ms"], color=color, alpha=0.8, linewidth=1.2)
        axes[2, 1].set_ylabel("P50 Latency (ms)")
        axes[2, 1].set_title("Median Task Latency")
        axes[2, 1].set_xlabel("Time (s)")

    fig.suptitle(title, fontsize=14, fontweight='bold')
    plt.tight_layout()
    plt.savefig(save_path, bbox_inches='tight')
    plt.close()
    print(f"  Saved: {save_path}")


def plot_factor_decomposition(metrics: MetricsCollector, label: str, save_path: str):
    """单个算法的因子分解图"""
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        return

    arr = metrics.get_arrays()
    if not arr:
        return

    fig, axes = plt.subplots(2, 2, figsize=(14, 8))

    time = arr["time"]

    axes[0, 0].plot(time, arr["beta"], 'b-', alpha=0.8, linewidth=1.5, label='β (backlog)')
    axes[0, 0].set_ylabel("Factor Value")
    axes[0, 0].set_title("β — Backlog Factor")
    axes[0, 0].axhline(y=1.0, color='gray', linestyle='--', alpha=0.3)

    axes[0, 1].plot(time, arr["lambda_speed"], 'g-', alpha=0.8, linewidth=1.5, label='λ (speed)')
    axes[0, 1].set_ylabel("Factor Value")
    axes[0, 1].set_title("λ — Speed Factor")

    axes[1, 0].plot(time, arr["sigma_volume"], 'orange', alpha=0.8, linewidth=1.5, label='σ (volume)')
    axes[1, 0].set_ylabel("Factor Value")
    axes[1, 0].set_title("σ — Volume Factor")

    axes[1, 1].plot(time, arr["gamma_variance"], 'r-', alpha=0.8, linewidth=1.5, label='γ (variance)')
    axes[1, 1].set_ylabel("Factor Value")
    axes[1, 1].set_title("γ — Variance Factor")

    fig.suptitle(f"Factor Decomposition — {label}", fontsize=14, fontweight='bold')
    plt.tight_layout()
    plt.savefig(save_path, bbox_inches='tight')
    plt.close()
    print(f"  Saved: {save_path}")


def plot_heatmap(results: Dict[str, MetricsCollector],
                 title: str, save_path: str):
    """算法 × 指标热力图"""
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        return

    labels = list(results.keys())
    metrics_names = ["Throughput", "OOM Rate", "Avg Latency", "P99 Latency", "Avg N_batch", "Utilization"]
    data = np.zeros((len(labels), len(metrics_names)))

    for i, (label, m) in enumerate(results.items()):
        s = m.summary()
        data[i, 0] = s.get("throughput_per_sec", 0)
        data[i, 1] = s.get("oom_rate", 0) * 100
        data[i, 2] = s.get("avg_latency_ms", 0)
        data[i, 3] = s.get("p99_latency_ms", 0)
        data[i, 4] = s.get("avg_n_batch", 0)
        data[i, 5] = s.get("avg_utilization", 0) * 100

    # 归一化每列到 [0,1]
    data_norm = np.zeros_like(data)
    for j in range(data.shape[1]):
        col = data[:, j]
        if col.max() > col.min():
            # OOM rate 和 latency 是越小越好 → 反转
            if j in (1, 2, 3):
                data_norm[:, j] = 1.0 - (col - col.min()) / (col.max() - col.min() + 1e-9)
            else:
                data_norm[:, j] = (col - col.min()) / (col.max() - col.min() + 1e-9)
        else:
            data_norm[:, j] = 0.5

    fig, ax = plt.subplots(figsize=(10, max(5, len(labels) * 0.5)))
    im = ax.imshow(data_norm, cmap='RdYlGn', aspect='auto', vmin=0, vmax=1)

    ax.set_xticks(range(len(metrics_names)))
    ax.set_xticklabels(metrics_names, rotation=30, ha='right')
    ax.set_yticks(range(len(labels)))
    ax.set_yticklabels(labels, fontsize=8)

    # 标注原始值
    for i in range(len(labels)):
        for j in range(len(metrics_names)):
            fmt = f'{data[i,j]:.1f}' if j != 1 else f'{data[i,j]:.2f}%'
            text_color = 'white' if data_norm[i,j] < 0.3 or data_norm[i,j] > 0.7 else 'black'
            ax.text(j, i, fmt, ha='center', va='center', fontsize=7,
                    color=text_color, fontweight='bold')

    ax.set_title(title, fontsize=14, fontweight='bold')
    plt.colorbar(im, ax=ax, label='Normalized Score')
    plt.tight_layout()
    plt.savefig(save_path, bbox_inches='tight')
    plt.close()
    print(f"  Saved: {save_path}")


def plot_pareto_frontier(results: Dict[str, MetricsCollector],
                          title: str, save_path: str):
    """帕累托前沿：吞吐量 vs OOM 率"""
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        return

    labels = list(results.keys())
    throughputs = [r.summary().get("throughput_per_sec", 0) for r in results.values()]
    oom_rates = [r.summary().get("oom_rate", 0) * 100 for r in results.values()]
    avg_n = [r.summary().get("avg_n_batch", 0) for r in results.values()]

    fig, ax = plt.subplots(figsize=(10, 7))

    scatter = ax.scatter(throughputs, oom_rates, c=avg_n, cmap='viridis',
                          s=150, alpha=0.8, edgecolors='black', linewidth=1)

    for i, label in enumerate(labels):
        ax.annotate(label, (throughputs[i], oom_rates[i]),
                    textcoords="offset points", xytext=(5, 5),
                    fontsize=7, alpha=0.9)

    ax.set_xlabel("Throughput (tasks/s)")
    ax.set_ylabel("OOM Rate (%)")
    ax.set_title(f"Pareto Frontier: Throughput vs OOM Rate\n{title}")
    ax.axhline(y=2.0, color='red', linestyle='--', alpha=0.5, label='2% OOM threshold')

    # 帕累托前沿线（注释掉连线的，因为不是真正的排序）
    cbar = plt.colorbar(scatter, ax=ax, label='Avg N_batch')
    ax.legend(fontsize=8)
    ax.grid(True, alpha=0.3)

    plt.tight_layout()
    plt.savefig(save_path, bbox_inches='tight')
    plt.close()
    print(f"  Saved: {save_path}")


def plot_dashboard(all_scenario_results: Dict[str, Dict[str, MetricsCollector]],
                   save_path: str):
    """汇总 Dashboard"""
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        return

    scenario_names = list(all_scenario_results.keys())
    n_scenarios = len(scenario_names)

    if n_scenarios == 0:
        return

    fig, axes = plt.subplots(n_scenarios, 2, figsize=(14, 4 * n_scenarios))
    if n_scenarios == 1:
        axes = axes.reshape(1, -1)

    for row, sname in enumerate(scenario_names):
        results = all_scenario_results[sname]
        labels = list(results.keys())
        tp = [r.summary().get("throughput_per_sec", 0) for r in results.values()]
        oom = [r.summary().get("oom_rate", 0) * 100 for r in results.values()]

        colors = plt.cm.tab10(np.linspace(0, 1, len(labels)))

        x = range(len(labels))

        # 吞吐量
        axes[row, 0].bar(x, tp, color=colors)
        axes[row, 0].set_xticks(x)
        axes[row, 0].set_xticklabels(labels, rotation=45, ha='right', fontsize=6)
        axes[row, 0].set_ylabel("Throughput")
        axes[row, 0].set_title(f"{sname} — Throughput")

        # OOM 率
        axes[row, 1].bar(x, oom, color=colors)
        axes[row, 1].set_xticks(x)
        axes[row, 1].set_xticklabels(labels, rotation=45, ha='right', fontsize=6)
        axes[row, 1].set_ylabel("OOM Rate (%)")
        axes[row, 1].set_title(f"{sname} — OOM Rate")
        axes[row, 1].axhline(y=2.0, color='red', linestyle='--', alpha=0.5)

    fig.suptitle("Algorithm Comparison Dashboard", fontsize=16, fontweight='bold')
    plt.tight_layout()
    plt.savefig(save_path, bbox_inches='tight')
    plt.close()
    print(f"  Saved: {save_path}")


def plot_new_factors(arrays: Dict[str, np.ndarray], save_path: str):
    """Plot θ confidence factor, prefetch hit rate, defrag merges."""
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib not installed, skipping plot_new_factors")
        return

    fig, axes = plt.subplots(3, 1, figsize=(10, 8), sharex=True)

    if 'theta_confidence' in arrays:
        axes[0].plot(arrays['time'], arrays['theta_confidence'], label='θ (Confidence)', color='purple')
        axes[0].set_ylabel('Confidence Factor')
        axes[0].legend(); axes[0].grid(True)

    if 'prefetch_hit_rate' in arrays:
        axes[1].plot(arrays['time'], arrays['prefetch_hit_rate'], label='Prefetch Hit Rate', color='orange')
        axes[1].set_ylabel('Hit Rate')
        axes[1].legend(); axes[1].grid(True)

    if 'cold_start_phase' in arrays:
        # Convert phase string to numeric for plotting
        phases = {'bootstrap': 0, 'warmup': 1, 'accumulate': 2, 'stable': 3}
        phase_nums = [phases.get(p, 3) for p in arrays['cold_start_phase']]
        axes[2].plot(arrays['time'], phase_nums, label='Cold Start Phase', color='red', drawstyle='steps-post')
        axes[2].set_yticks([0, 1, 2, 3])
        axes[2].set_yticklabels(['Bootstrap', 'Warmup', 'Accumulate', 'Stable'])
        axes[2].set_ylabel('Phase')
        axes[2].legend(); axes[2].grid(True)

    axes[-1].set_xlabel('Time (s)')
    plt.tight_layout()
    plt.savefig(save_path, dpi=150)
    plt.close()
    print(f"  Saved: {save_path}")


def plot_load_distribution(arrays: Dict[str, np.ndarray], save_path: str):
    """Plot load balance metric over time."""
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib not installed, skipping plot_load_distribution")
        return

    if 'balance_metric' not in arrays:
        return
    fig, ax = plt.subplots(figsize=(10, 4))
    ax.plot(arrays['time'], arrays['balance_metric'], label='Load Balance', color='green')
    ax.axhline(y=1.0, color='gray', linestyle='--', alpha=0.5, label='Perfect Balance')
    ax.set_ylabel('Balance Metric')
    ax.set_xlabel('Time (s)')
    ax.legend(); ax.grid(True)
    plt.tight_layout()
    plt.savefig(save_path, dpi=150)
    plt.close()
    print(f"  Saved: {save_path}")
