"""
生成全中文科学图表 —— Atomix 策略模块
使用微软雅黑字体，所有标签为中文。
"""

import json
import os
import warnings
import numpy as np

warnings.filterwarnings('ignore')
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.font_manager as fm
import matplotlib.ticker as mticker
import seaborn as sns

# ── 中文字体设置 ──
# 查找可用的中文字体
_available = [f.name for f in fm.fontManager.ttflist]
if 'Microsoft YaHei' in _available:
    _CN_FONT = 'Microsoft YaHei'
elif 'Noto Sans SC' in _available:
    _CN_FONT = 'Noto Sans SC'
elif 'SimHei' in _available:
    _CN_FONT = 'SimHei'
else:
    _CN_FONT = 'sans-serif'
    print("警告：未找到中文字体，图表中文可能显示为方块")

plt.rcParams.update({
    'font.family': 'sans-serif',
    'font.sans-serif': [_CN_FONT, 'DejaVu Sans', 'Arial'],
    'axes.unicode_minus': False,   # 解决负号显示问题
    'figure.dpi': 200,
    'savefig.dpi': 200,
    'font.size': 12,
    'axes.titlesize': 15,
    'axes.labelsize': 13,
    'legend.fontsize': 10,
    'figure.facecolor': 'white',
    'axes.facecolor': '#f8f9fa',
    'axes.edgecolor': '#cccccc',
    'axes.grid': True,
    'grid.alpha': 0.3,
    'grid.color': '#cccccc',
})
sns.set_palette("Set2")

ASSETS_DIR = ".assets"
REPORTS_DIR = "sim/reports"
os.makedirs(ASSETS_DIR, exist_ok=True)

print(f"使用字体: {_CN_FONT}")


def load_all_data():
    data = {}
    with open(os.path.join(REPORTS_DIR, "master_summary.json"), "r", encoding="utf-8") as f:
        data["master"] = json.load(f)
    for sname in os.listdir(REPORTS_DIR):
        sdir = os.path.join(REPORTS_DIR, sname)
        if os.path.isdir(sdir):
            sf = os.path.join(sdir, "summary.json")
            if os.path.exists(sf):
                with open(sf, "r", encoding="utf-8") as f:
                    data[sname] = json.load(f)
    return data


# ═══════════════════════════════════════════════════════════════
# 图1: 算法性能热力矩阵
# ═══════════════════════════════════════════════════════════════

def chart_heatmap(data):
    master = data["master"]
    scenarios = list(master.keys())
    algorithms = list(master[scenarios[0]].keys())
    baseline_tp = {a: master[scenarios[0]][a].get("throughput_per_sec", 0) for a in algorithms}
    algorithms.sort(key=lambda a: ("Baseline" in a, -baseline_tp[a]))

    # 场景名称简写
    short_names = {
        "稳态混合负载": "稳态混合",
        "突发冲击": "突发冲击", 
        "内存压力": "内存压力",
        "CPU压力": "CPU压力",
        "震荡测试": "震荡测试",
        "长跑稳定性": "长跑稳定",
    }
    scen_labels = [short_names.get(s, s[:6]) for s in scenarios]

    metrics_cn = [
        ("throughput_per_sec", "吞吐量 (任务/秒)", "RdYlGn", False),
        ("oom_rate", "OOM 率 (%)", "RdYlGn_r", True),
        ("avg_latency_ms", "平均延迟 (毫秒)", "RdYlGn_r", True),
        ("p99_latency_ms", "P99 延迟 (毫秒)", "RdYlGn_r", True),
        ("avg_n_batch", "平均批次额度", "RdYlGn", False),
        ("avg_utilization", "槽位利用率 (%)", "RdYlGn", False),
    ]

    fig, axes = plt.subplots(2, 3, figsize=(20, 11))

    for idx, (metric, title, cmap, invert) in enumerate(metrics_cn):
        ax = axes[idx // 3, idx % 3]
        mat = np.zeros((len(algorithms), len(scenarios)))
        for i, algo in enumerate(algorithms):
            for j, scen in enumerate(scenarios):
                val = master[scen][algo].get(metric, 0)
                if metric == "oom_rate":
                    val *= 100
                if metric == "avg_utilization":
                    val *= 100
                mat[i, j] = val

        mat_norm = np.zeros_like(mat)
        for j in range(len(scenarios)):
            col = mat[:, j]
            if col.max() > col.min():
                if invert:
                    mat_norm[:, j] = 1.0 - (col - col.min()) / (col.max() - col.min() + 1e-9)
                else:
                    mat_norm[:, j] = (col - col.min()) / (col.max() - col.min() + 1e-9)
            else:
                mat_norm[:, j] = 0.5

        im = ax.imshow(mat_norm, cmap=cmap, aspect='auto', vmin=0, vmax=1)

        for i in range(len(algorithms)):
            for j in range(len(scenarios)):
                val = mat[i, j]
                if metric in ("oom_rate",):
                    fmt = f'{val:.1f}%'
                elif metric in ("avg_utilization",):
                    fmt = f'{val:.0f}%'
                elif val >= 100:
                    fmt = f'{val:.0f}'
                else:
                    fmt = f'{val:.1f}'
                tc = 'white' if mat_norm[i, j] < 0.3 or mat_norm[i, j] > 0.7 else 'black'
                ax.text(j, i, fmt, ha='center', va='center', fontsize=8, color=tc, fontweight='bold')

        ax.set_xticks(range(len(scenarios)))
        ax.set_xticklabels(scen_labels, rotation=30, ha='right', fontsize=9)
        ax.set_yticks(range(len(algorithms)))
        ax.set_yticklabels(algorithms, fontsize=9)
        ax.set_title(title, fontweight='bold', fontsize=14)
        plt.colorbar(im, ax=ax, fraction=0.046)

    fig.suptitle("算法性能热力矩阵 — 全部场景 × 全部变体", fontsize=18, fontweight='bold', y=1.01)
    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "heatmap_matrix.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图2: 帕累托前沿
# ═══════════════════════════════════════════════════════════════

def chart_pareto(data):
    master = data["master"]
    fig, ax = plt.subplots(figsize=(13, 9))
    colors = plt.cm.tab10(np.linspace(0, 1, len(master)))
    markers = ['o', 's', 'D', '^', 'v', 'P']
    first_seen = set()

    for idx, (sname, results) in enumerate(master.items()):
        for algo, metrics in results.items():
            tp = metrics["throughput_per_sec"]
            oom = metrics["oom_rate"] * 100
            is_bl = "Baseline" in algo
            label = sname if sname not in first_seen else ""
            first_seen.add(sname)

            ax.scatter(tp, oom, c=[colors[idx]], marker=markers[idx],
                      s=220 if is_bl else 130, alpha=0.85,
                      edgecolors='black' if is_bl else 'white',
                      linewidth=1.8 if is_bl else 0.5,
                      zorder=6 if is_bl else 3)

            if is_bl or oom < 2.0:
                short = algo.split("(")[0].strip() if "(" in algo else algo[:14]
                ax.annotate(short, (tp, oom), textcoords="offset points",
                          xytext=(6, 6), fontsize=8, alpha=0.9)

    ax.axhline(y=2.0, color='red', linestyle='--', alpha=0.6, linewidth=1.5, label='2% OOM 警戒线')
    ax.set_xlabel("吞吐量 (任务/秒)", fontsize=14)
    ax.set_ylabel("OOM 率 (%)", fontsize=14)
    ax.set_title("帕累托前沿：吞吐量 vs OOM 率\n（右下角更优）", fontsize=17, fontweight='bold')

    from matplotlib.patches import Patch
    legend_elements = [Patch(facecolor=colors[i], label=s, alpha=0.8) for i, s in enumerate(master.keys())]
    ax.legend(handles=legend_elements + [plt.Line2D([0], [0], color='red', linestyle='--', label='2% OOM 警戒线')],
             loc='upper left', fontsize=9)
    ax.set_xlim(left=0)
    ax.set_ylim(bottom=-0.5)

    path = os.path.join(ASSETS_DIR, "pareto_frontier.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图3: 各场景 OOM率/吞吐量/延迟 分组对比
# ═══════════════════════════════════════════════════════════════

def chart_oom_comparison(data):
    master = data["master"]
    scenarios = list(master.keys())
    short_names = {
        "稳态混合负载": "稳态混合", "突发冲击": "突发冲击",
        "内存压力": "内存压力", "CPU压力": "CPU压力",
        "震荡测试": "震荡测试", "长跑稳定性": "长跑稳定",
    }

    key_algos = [
        "Baseline (Disc+Mul+Hard+1.5x)",
        "Sigmoid Only",
        "AIMD+Hysteresis",
        "FullOpt",
        "FullAdaptive",
    ]
    available = [a for a in key_algos if a in master[scenarios[0]]]
    short_algo = ["基准(原文档)", "仅Sigmoid", "AIMD+滞回", "全优化", "全自适应"]

    fig, axes = plt.subplots(1, 3, figsize=(20, 6.5))
    colors = sns.color_palette("Set2", len(scenarios))

    x = np.arange(len(available))
    width = 0.13

    metric_configs = [
        ("oom_rate", 100, "OOM 率 (%)", "越低越好"),
        ("throughput_per_sec", 1, "吞吐量 (任务/秒)", "越高越好"),
        ("p99_latency_ms", 1, "P99 延迟 (毫秒)", "越低越好"),
    ]

    for sub_idx, (metric, scale, ylabel, tag) in enumerate(metric_configs):
        ax = axes[sub_idx]
        for s_idx, sname in enumerate(scenarios):
            values = [master[sname][a].get(metric, 0) * scale for a in available]
            offset = (s_idx - len(scenarios)/2 + 0.5) * width
            bars = ax.bar(x + offset, values, width * 0.9, color=colors[s_idx % len(colors)],
                         alpha=0.85, label=short_names.get(sname, sname))

        ax.set_xticks(x)
        ax.set_xticklabels(short_algo, rotation=25, ha='right', fontsize=10)
        ax.set_ylabel(ylabel, fontsize=13)
        ax.set_title(f"{ylabel}\n（{tag}）", fontweight='bold', fontsize=14)
        if sub_idx == 1:
            ax.legend(fontsize=8, loc='upper right', ncol=3)

    fig.suptitle("算法性能对比 — 全部场景", fontsize=18, fontweight='bold')
    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "oom_throughput_comparison.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图4: FullAdaptive vs Baseline 改进雷达图
# ═══════════════════════════════════════════════════════════════

def chart_improvement_radar(data):
    master = data["master"]
    scenarios = list(master.keys())
    short_names = {"稳态混合负载": "稳态混合", "突发冲击": "突发冲击",
                   "内存压力": "内存压力", "CPU压力": "CPU压力",
                   "震荡测试": "震荡测试", "长跑稳定性": "长跑稳定"}

    baseline = "Baseline (Disc+Mul+Hard+1.5x)"
    optimized = "FullAdaptive"
    if baseline not in master[scenarios[0]] or optimized not in master[scenarios[0]]:
        print("  跳过雷达图——缺少必要算法")
        return

    metrics_cn = [
        ("吞吐量", "throughput_per_sec", 1, True),
        ("零OOM", "oom_rate", 100, False),
        ("平均延迟", "avg_latency_ms", 1, False),
        ("P99延迟", "p99_latency_ms", 1, False),
        ("批次额度", "avg_n_batch", 1, True),
    ]

    fig, ax = plt.subplots(figsize=(11, 11), subplot_kw=dict(polar=True))
    colors = plt.cm.tab10(np.linspace(0, 1, len(scenarios)))
    angles = np.linspace(0, 2 * np.pi, len(metrics_cn), endpoint=False).tolist()
    angles += angles[:1]

    for s_idx, sname in enumerate(scenarios):
        values = []
        for mname, metric, scale, higher_better in metrics_cn:
            base_val = master[sname][baseline].get(metric, 0) * scale
            opt_val = master[sname][optimized].get(metric, 0) * scale
            if base_val == 0:
                pct = 100
            elif higher_better:
                pct = (opt_val / base_val) * 100 if base_val > 0 else 100
            else:
                pct = max(0, (1.0 - (opt_val / max(base_val, 1e-9))) * 100 + 100)
            values.append(max(0, min(300, pct)))
        values += values[:1]

        ax.fill(angles, values, alpha=0.12, color=colors[s_idx])
        ax.plot(angles, values, 'o-', linewidth=2.2, color=colors[s_idx],
               label=short_names.get(sname, sname), markersize=7)

    ax.set_xticks(angles[:-1])
    ax.set_xticklabels([m[0] for m in metrics_cn], fontsize=12)
    ax.set_ylim(0, 220)
    ax.axhline(y=100, color='gray', linestyle='--', alpha=0.5, linewidth=1)
    ax.set_title("全自适应 vs 原文档基准（改进百分比）\n100% = 持平，>100% = 更优", fontsize=16, fontweight='bold', pad=22)
    ax.legend(loc='upper right', bbox_to_anchor=(1.35, 1.1), fontsize=10)

    path = os.path.join(ASSETS_DIR, "improvement_radar.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图5: 因子平滑化对比
# ═══════════════════════════════════════════════════════════════

def chart_factor_behavior(data):
    fig, axes = plt.subplots(2, 2, figsize=(15, 11))
    k = 5.0

    # β: 积压因子
    d_vals = np.linspace(0, 5, 500)
    beta_old = np.piecewise(d_vals, [d_vals < 1, (d_vals >= 1) & (d_vals < 2),
                                      (d_vals >= 2) & (d_vals < 3), d_vals >= 3],
                            [1.0, 0.85, 0.70, 0.50])
    beta_new = 0.50 + 0.50 / (1 + np.exp(k * (d_vals - 1.5)))
    axes[0, 0].step(d_vals, beta_old, 'r-', linewidth=2, alpha=0.7, label='离散分段（旧）', where='post')
    axes[0, 0].plot(d_vals, beta_new, 'b-', linewidth=2.8, label='S型连续（新）')
    axes[0, 0].axvline(x=1.0, color='gray', linestyle=':', alpha=0.4)
    axes[0, 0].axvline(x=2.0, color='gray', linestyle=':', alpha=0.4)
    axes[0, 0].set_xlabel("积压比 d = 池深 / H")
    axes[0, 0].set_ylabel("β(d)")
    axes[0, 0].set_title("积压因子 β(d)", fontweight='bold', fontsize=14)
    axes[0, 0].legend(fontsize=11)
    axes[0, 0].set_ylim(0.4, 1.1)

    # λ: 速度因子
    mu_vals = np.linspace(0, 5000, 500)
    lambda_old = np.piecewise(mu_vals, [mu_vals < 50, (mu_vals >= 50) & (mu_vals < 200),
                                         (mu_vals >= 200) & (mu_vals < 1000), mu_vals >= 1000],
                              [1.40, 1.20, 1.10, 1.00])
    lambda_new = 1.00 + 0.40 / (1 + np.exp(k * (mu_vals / 500 - 1.0)))
    axes[0, 1].step(mu_vals, lambda_old, 'r-', linewidth=2, alpha=0.7, label='离散分段（旧）', where='post')
    axes[0, 1].plot(mu_vals, lambda_new, 'b-', linewidth=2.8, label='S型连续（新）')
    axes[0, 1].set_xlabel("平均任务耗时 μ_t (毫秒)")
    axes[0, 1].set_ylabel("λ(μ_t)")
    axes[0, 1].set_title("速度因子 λ(μ_t)", fontweight='bold', fontsize=14)
    axes[0, 1].legend(fontsize=11)
    axes[0, 1].set_ylim(0.9, 1.5)

    # σ: 体积因子
    r_vals = np.linspace(0, 5, 500)
    sigma_old = np.piecewise(r_vals, [r_vals < 0.3, (r_vals >= 0.3) & (r_vals < 0.6),
                                       (r_vals >= 0.6) & (r_vals < 1.5),
                                       (r_vals >= 1.5) & (r_vals < 3.0), r_vals >= 3.0],
                              [1.30, 1.15, 1.00, 0.80, 0.60])
    sigma_new = 0.55 + 0.80 / (1 + np.exp(k * (r_vals - 1.0)))
    axes[1, 0].step(r_vals, sigma_old, 'r-', linewidth=2, alpha=0.7, label='离散分段（旧）', where='post')
    axes[1, 0].plot(r_vals, sigma_new, 'b-', linewidth=2.8, label='S型连续（新）')
    axes[1, 0].axvline(x=1.0, color='gray', linestyle=':', alpha=0.4, label='预估值')
    axes[1, 0].set_xlabel("内存比 r = μ_m / 预估值")
    axes[1, 0].set_ylabel("σ(r)")
    axes[1, 0].set_title("体积因子 σ(r)", fontweight='bold', fontsize=14)
    axes[1, 0].legend(fontsize=11)
    axes[1, 0].set_ylim(0.4, 1.5)

    # γ: 方差因子
    v_vals = np.linspace(0, 2, 500)
    gamma_old = np.piecewise(v_vals, [v_vals < 0.3, (v_vals >= 0.3) & (v_vals < 0.6),
                                       (v_vals >= 0.6) & (v_vals < 1.0), v_vals >= 1.0],
                              [1.00, 0.85, 0.70, 0.55])
    gamma_new = 0.50 + 0.55 / (1 + np.exp(k * (v_vals - 0.5)))
    axes[1, 1].step(v_vals, gamma_old, 'r-', linewidth=2, alpha=0.7, label='离散分段（旧）', where='post')
    axes[1, 1].plot(v_vals, gamma_new, 'b-', linewidth=2.8, label='S型连续（新）')
    axes[1, 1].set_xlabel("变异系数 v_t = σ_t / μ_t")
    axes[1, 1].set_ylabel("γ(v_t)")
    axes[1, 1].set_title("方差因子 γ(v_t)", fontweight='bold', fontsize=14)
    axes[1, 1].legend(fontsize=11)
    axes[1, 1].set_ylim(0.4, 1.2)

    fig.suptitle("因子平滑化：离散分段（红色）→ 连续S型函数（蓝色）", fontsize=17, fontweight='bold')
    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "factor_smoothing.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图6: OOM 反馈策略对比
# ═══════════════════════════════════════════════════════════════

def chart_oom_feedback_comparison(data):
    fig, axes = plt.subplots(1, 3, figsize=(19, 5.5))
    np.random.seed(42)
    time = np.arange(0, 200, 0.1)

    oom_events = []
    for t in time:
        if np.random.random() < 0.03:
            oom_events.append(t)
    window = 60.0

    # 策略1: 硬乘除
    alpha_hard = np.ones_like(time) * 0.5
    evts = list(oom_events)
    for i, t in enumerate(time):
        if i > 0:
            alpha_hard[i] = alpha_hard[i-1]
        recent = sum(1 for e in evts if t - window < e <= t)
        if recent >= 3:
            alpha_hard[i] *= 0.8
            evts = [e for e in evts if e <= t - window]
        elif recent == 0 and alpha_hard[i] < 0.5:
            if i % 600 == 0:
                alpha_hard[i] = min(0.5, alpha_hard[i] * 1.1)

    # 策略2: AIMD 无滞回
    alpha_aimd = np.ones_like(time) * 0.5
    evts2 = list(oom_events)
    for i, t in enumerate(time):
        if i > 0:
            alpha_aimd[i] = alpha_aimd[i-1]
        recent = sum(1 for e in evts2 if t - window < e <= t)
        if recent >= 3:
            alpha_aimd[i] *= 0.75
            evts2 = [e for e in evts2 if e <= t - window]
        elif recent == 0 and alpha_aimd[i] < 0.5:
            alpha_aimd[i] += 0.003

    # 策略3: AIMD + 滞回
    alpha_hyst = np.ones_like(time) * 0.5
    evts3 = list(oom_events)
    for i, t in enumerate(time):
        if i > 0:
            alpha_hyst[i] = alpha_hyst[i-1]
        recent = sum(1 for e in evts3 if t - window < e <= t)
        if recent >= 5:
            alpha_hyst[i] *= 0.75
            evts3 = [e for e in evts3 if e <= t - window]
        elif recent <= 2 and alpha_hyst[i] < 0.5:
            alpha_hyst[i] += 0.0015

    configs = [
        (alpha_hard, "硬乘除（旧方案）\n≥3次→×0.8 / 60秒无OOM→×1.1", '#e74c3c'),
        (alpha_aimd, "AIMD（无滞回）\n≥3次→×0.75 / 正常→+0.03/秒", '#f39c12'),
        (alpha_hyst, "AIMD + 滞回（推荐）\n≥5次→×0.75 / ≤2次→+0.015/秒 / [3,4]死区", '#27ae60'),
    ]

    for ax, (alpha, title, color) in zip(axes, configs):
        ax.plot(time, alpha, color=color, linewidth=2.2)
        ax.axhline(y=0.5, color='gray', linestyle='--', alpha=0.5, label='初始 α_mem = 0.5')
        ax.set_xlabel("时间 (秒)", fontsize=12)
        ax.set_ylabel("α_mem", fontsize=12)
        ax.set_title(title, fontweight='bold', fontsize=12)
        ax.set_ylim(0.1, 0.6)
        ax.legend(fontsize=9)

    fig.suptitle("OOM 反馈策略对比（相同 OOM 事件序列，不同响应行为）", fontsize=16, fontweight='bold')
    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "oom_feedback_comparison.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图7: 俄罗斯方块滑道机制
# ═══════════════════════════════════════════════════════════════

def chart_slipway_viz(data):
    fig, axes = plt.subplots(2, 3, figsize=(20, 9))

    stages = [
        {"slots": [(0, 100, "槽0", "#3498db"), (100, 100, "槽1", "#3498db"),
                   (200, 100, "槽2", "#3498db"), (300, 100, "槽3", "#3498db")],
         "slipway": (400, 150), "title": "① 正常运行\n4槽位 + 1.5倍滑道"},
        {"slots": [(0, 100, "槽0", "#3498db"), (100, 100, "槽1⚠", "#e74c3c"),
                   (200, 100, "槽2", "#3498db"), (300, 100, "槽3", "#3498db")],
         "slipway": (400, 150), "title": "② 槽1 OOM\n需要扩容"},
        {"slots": [(0, 100, "槽0", "#3498db"), (100, 100, "死区", "#95a5a6"),
                   (200, 100, "槽2", "#3498db"), (300, 100, "槽3", "#3498db")],
         "slipway": (400, 150), "expanded": (400, 120, "槽1'", "#e67e22"),
         "title": "③ 槽1滑入滑道\n原位置变死区"},
        {"slots": [(0, 100, "槽0", "#3498db"), (100, 200, "空闲", "#2ecc71"),
                   (300, 100, "槽3", "#3498db")],
         "slipway": (400, 80), "expanded": (400, 80, "槽1'", "#e67e22"),
         "title": "④ 槽2完成→死区合并\n释放2倍槽位空间"},
        {"slots": [(0, 100, "槽0", "#3498db"), (100, 100, "槽4", "#9b59b6"),
                   (200, 100, "槽3", "#3498db")],
         "slipway": (300, 250), "expanded": (300, 80, "槽1'", "#e67e22"),
         "title": "⑤ 新建槽4\n滑道恢复正常大小"},
        {"slots": [(0, 100, "槽0", "#3498db"), (100, 100, "槽4", "#9b59b6"),
                   (200, 100, "槽3", "#3498db"), (300, 150, "槽5", "#1abc9c")],
         "slipway": (450, 100), "title": "⑥ 槽1'完成→全部回收\n4槽 + 滑道恢复"},
    ]

    for idx, stage in enumerate(stages):
        ax = axes[idx // 3, idx % 3]
        ax.set_xlim(0, 600)
        ax.set_ylim(0, 1)

        for x, w, label, color in stage["slots"]:
            rect = plt.Rectangle((x, 0.1), w, 0.5, linewidth=1.5, edgecolor='white',
                                facecolor=color, alpha=0.85)
            ax.add_patch(rect)
            tc = 'white' if color not in ('#2ecc71',) else '#155724'
            ax.text(x + w/2, 0.35, label, ha='center', va='center', fontsize=9, fontweight='bold', color=tc)

        sx, sw = stage["slipway"]
        rect = plt.Rectangle((sx, 0.1), sw, 0.5, linewidth=1.8, edgecolor='#27ae60',
                            facecolor='#a8e6cf', alpha=0.55, linestyle='--')
        ax.add_patch(rect)
        ax.text(sx + sw/2, 0.35, "滑道", ha='center', va='center', fontsize=9, color='#155724', fontstyle='italic')

        if "expanded" in stage:
            ex, ew, elabel, ecolor = stage["expanded"]
            rect = plt.Rectangle((ex, 0.15), ew, 0.4, linewidth=1.5, edgecolor='white',
                                facecolor=ecolor, alpha=0.85)
            ax.add_patch(rect)
            ax.text(ex + ew/2, 0.35, elabel, ha='center', va='center', fontsize=9, fontweight='bold', color='white')

        ax.set_title(stage["title"], fontsize=11, fontweight='bold')
        ax.axis('off')

    fig.suptitle("俄罗斯方块内存管理：滑道扩容与回收机制", fontsize=17, fontweight='bold')
    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "slipway_mechanism.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图8: 因子合并策略对比
# ═══════════════════════════════════════════════════════════════

def chart_merge_comparison(data):
    fig, ax = plt.subplots(figsize=(13, 7.5))
    factors = np.array([0.7, 0.7, 0.7, 1.0])
    labels_cn = ['β 积压', 'λ 速度', 'σ 体积', 'γ 方差']
    x = np.arange(4)
    width = 0.25
    colors = ['#e74c3c', '#3498db', '#2ecc71', '#f39c12']

    bars = ax.bar(x - width, factors, width * 0.9, color=colors, alpha=0.75, label='各因子原始值')
    for i, (bar, val) in enumerate(zip(bars, factors)):
        ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.015,
               f'{val:.1f}', ha='center', fontsize=11, fontweight='bold')

    merge_data = [
        ("乘法链\nβ×λ×σ×γ", 0.343, '#e74c3c'),
        ("取最小值\nmin()", 0.70, '#f39c12'),
        ("加权几何平均\nexp(Σw·ln(f))", 0.756, '#27ae60'),
    ]

    for i, (label, val, color) in enumerate(merge_data):
        bar = ax.bar(2.5 + i * 1.3, val, 1.0, color=color, alpha=0.85,
                    edgecolor='black', linewidth=1.8)
        ax.text(bar[0].get_x() + bar[0].get_width()/2, bar[0].get_height() + 0.015,
               f'{val:.2f}', ha='center', fontsize=13, fontweight='bold')
        ax.text(bar[0].get_x() + bar[0].get_width()/2, 0.04, label,
               ha='center', fontsize=9, color='#333333')

    ax.axhline(y=1.0, color='gray', linestyle='--', alpha=0.5, label='无调整 (1.0)')
    ax.set_xticks(list(x) + [3.15, 4.45, 5.75])
    ax.set_xticklabels(labels_cn + ['乘法链', '最小值', '加权几何'], fontsize=11)
    ax.set_ylabel("综合调整因子", fontsize=14)
    ax.set_title("因子合并策略对比\n（三个因子 = 0.7，一个因子 = 1.0）", fontsize=16, fontweight='bold')
    ax.legend(fontsize=10)
    ax.set_ylim(0, 1.35)

    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "merge_strategy_comparison.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图9: 指令编码位域图（5 种模板）
# ═══════════════════════════════════════════════════════════════

def chart_instruction_encodings(data=None):
    """绘制 32 位指令的 5 种编码模板位域图"""
    encodings = [
        ("32 位指令总格式", [
            ("操作数域 (Operand Field)", 24, "#3498db"),
            ("Opcode", 8, "#e74c3c"),
        ]),
        ("R3 模板 (三寄存器 + funct)", [
            ("rd [4]", 4, "#2ecc71"),
            ("rs1 [4]", 4, "#27ae60"),
            ("rs2 [4]", 4, "#1abc9c"),
            ("funct [12]", 12, "#f39c12"),
            ("Opcode [8]", 8, "#e74c3c"),
        ]),
        ("R2I 模板 (双寄存器 + 立即数)", [
            ("rd [4]", 4, "#2ecc71"),
            ("rs1 [4]", 4, "#27ae60"),
            ("imm [16]", 16, "#9b59b6"),
            ("Opcode [8]", 8, "#e74c3c"),
        ]),
        ("R1I 模板 (单寄存器 + 立即数)", [
            ("rd [4]", 4, "#2ecc71"),
            ("imm [20]", 20, "#9b59b6"),
            ("Opcode [8]", 8, "#e74c3c"),
        ]),
        ("JI 模板 (纯地址/立即数)", [
            ("offset / imm [24]", 24, "#3498db"),
            ("Opcode [8]", 8, "#e74c3c"),
        ]),
    ]

    fig, axes = plt.subplots(len(encodings), 1, figsize=(18, 2.2 * len(encodings)))
    if len(encodings) == 1:
        axes = [axes]

    for idx, (title, fields) in enumerate(encodings):
        ax = axes[idx]
        ax.set_xlim(0, 32)
        ax.set_ylim(0, 1)

        x_start = 0
        for label, width, color in fields:
            rect = plt.Rectangle((x_start, 0.15), width, 0.7, linewidth=1.5,
                                edgecolor='white', facecolor=color, alpha=0.85)
            ax.add_patch(rect)
            # Label inside the block
            short_label = label.split("[")[0].strip() if "[" in label else label
            ax.text(x_start + width/2, 0.5, short_label, ha='center', va='center',
                   fontsize=11, fontweight='bold', color='white')
            # Bit range annotation above
            bit_hi = 31 - x_start
            bit_lo = 32 - x_start - width
            ax.text(x_start + width/2, 0.95, f'[{bit_hi}:{bit_lo}]', ha='center', va='bottom',
                   fontsize=8, color='#555555')
            # Width annotation below
            ax.text(x_start + width/2, 0.08, f'{width}b', ha='center', va='top',
                   fontsize=8, color='#555555')
            x_start += width

        # Bit ruler at bottom
        for b in range(0, 33, 4):
            ax.axvline(x=b, color='#cccccc', linestyle=':', alpha=0.3)

        ax.set_title(title, fontweight='bold', fontsize=13, loc='left')
        ax.axis('off')

    fig.suptitle("Atomix 指令编码模板 — 32 位定长指令", fontsize=17, fontweight='bold')
    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "instruction_encodings.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图10: 内存水位线示意
# ═══════════════════════════════════════════════════════════════

def chart_memory_watermark(data=None):
    """绘制任务内存空间的水位线示意"""
    fig, ax = plt.subplots(figsize=(8, 9))
    ax.set_xlim(0, 4)
    ax.set_ylim(0, 10)

    # Background: full memory space
    total = plt.Rectangle((1, 0), 2, 10, linewidth=2, edgecolor='#333333',
                         facecolor='#ecf0f1', alpha=0.3)
    ax.add_patch(total)

    # Used region (top)
    used = plt.Rectangle((1, 6.5), 2, 3.5, linewidth=0, facecolor='#e74c3c', alpha=0.6)
    ax.add_patch(used)
    ax.text(3.3, 8.25, "已使用区域\n(Used)", fontsize=12, fontweight='bold', color='#c0392b', va='center')

    # Safe zone
    safe = plt.Rectangle((1, 3.5), 2, 3.0, linewidth=0, facecolor='#2ecc71', alpha=0.4)
    ax.add_patch(safe)
    ax.text(3.3, 5.0, "安全区\n(Safe Zone)\n正常分配", fontsize=11, color='#27ae60', va='center')

    # Reserved zone
    reserved = plt.Rectangle((1, 0), 2, 3.5, linewidth=0, facecolor='#f39c12', alpha=0.3)
    ax.add_patch(reserved)
    ax.text(3.3, 2.0, "保留区\n(Reserved)\n不可分配\n供紧急操作使用", fontsize=10, color='#e67e22', va='center')

    # Watermark line
    ax.axhline(y=6.5, xmin=0.25, xmax=0.75, color='#e74c3c', linewidth=3, linestyle='-')
    ax.axhline(y=6.5, xmin=0.1, xmax=0.9, color='#e74c3c', linewidth=1, linestyle='--', alpha=0.5)
    ax.text(0.75, 6.5, "▲ 警戒线\n(Watermark)", fontsize=12, fontweight='bold', color='#c0392b',
           va='center', ha='right')

    # Boundary line between safe and reserved
    ax.axhline(y=3.5, xmin=0.25, xmax=0.75, color='#f39c12', linewidth=1.5, linestyle='--')
    ax.text(0.75, 3.5, "保留边界", fontsize=9, color='#e67e22', va='center', ha='right')

    # Annotations
    ax.text(2, -0.5, "每次 ECALL alloc / CALL / TASK_FORK 前检查水位",
           ha='center', fontsize=10, color='#555555', style='italic')

    ax.set_title("任务内存空间 — 水位线机制", fontsize=16, fontweight='bold')
    ax.axis('off')
    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "memory_watermark.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图11: VM 运行时架构总览
# ═══════════════════════════════════════════════════════════════

def chart_vm_architecture(data=None):
    """绘制 VM Runtime 核心组件架构图"""
    fig, ax = plt.subplots(figsize=(18, 11))
    ax.set_xlim(0, 16)
    ax.set_ylim(0, 10)
    ax.axis('off')

    def draw_box(x, y, w, h, label, color, fontsize=10, edgecolor=None):
        if edgecolor is None:
            edgecolor = color
        rect = plt.Rectangle((x, y), w, h, linewidth=2, edgecolor=edgecolor,
                            facecolor=color, alpha=0.15)
        ax.add_patch(rect)
        ax.text(x + w/2, y + h/2, label, ha='center', va='center',
               fontsize=fontsize, fontweight='bold', color=color, linespacing=1.3)

    def draw_arrow(x1, y1, x2, y2, color='#555555', lw=1.5):
        ax.annotate('', xy=(x2, y2), xytext=(x1, y1),
                   arrowprops=dict(arrowstyle='->', color=color, lw=lw,
                                  connectionstyle='arc3,rad=0'))

    # Title
    ax.text(8, 9.5, "VM Runtime 核心组件", ha='center', fontsize=18, fontweight='bold', color='#2c3e50')

    # Top row: Entry
    draw_box(0.5, 7.8, 3, 1.3, "任务进入\n(Task Entry)", "#2c3e50", 11)

    # Flow arrow down
    ax.annotate('', xy=(2, 7.8), xytext=(2, 9.1),
               arrowprops=dict(arrowstyle='->', color='#555555', lw=2))

    # Second row: Task Pool
    draw_box(0.5, 5.8, 3, 1.7, "任务池\n(Task Pool)\n[磁盘存储]", "#8e44ad", 11)

    # Main horizontal: Task Pool → Batch Manager → Executor
    draw_box(4.5, 5.8, 4, 1.7, "批次管理器\n(Batch Manager)\n硬上限 H + 软上限 S", "#2980b9", 11)
    draw_box(9.5, 5.8, 3.5, 1.7, "执行引擎\n(Executor)\n实地址执行", "#c0392b", 11)

    draw_arrow(3.5, 6.65, 4.5, 6.65, '#555555', 2)
    draw_arrow(8.5, 6.65, 9.5, 6.65, '#555555', 2)

    # Bottom row: Memory Manager, Risk Control
    draw_box(4.5, 3.0, 4, 1.7, "内存管理器\n(Memory Manager)\n虚地址分配 · 扩容 · 回收", "#27ae60", 11)
    draw_box(9.5, 3.0, 3.5, 1.7, "风险管控\n(Risk Control)\nOOM 反馈 · 计数", "#e67e22", 11)

    draw_arrow(6.5, 5.8, 6.5, 4.7, '#555555', 1.5)
    draw_arrow(11.25, 5.8, 11.25, 4.7, '#555555', 1.5)

    # Feedback: Risk Control ← Memory Manager
    ax.annotate('', xy=(9.5, 4.7), xytext=(8.5, 4.7),
               arrowprops=dict(arrowstyle='->', color='#e67e22', lw=1.5, connectionstyle='arc3,rad=0.3'))

    # Bottom sub-components
    draw_box(0.5, 1.5, 3, 1.2, "取指 / 解码\n(Fetch / Decode)", "#16a085", 9)
    draw_box(4.5, 1.5, 3, 1.2, "沙箱隔离\n(Sandbox)", "#2c3e50", 9)
    draw_box(8.5, 1.5, 3.5, 1.2, "Syscall Gateway\n(系统调用网关)", "#8e44ad", 9)

    # Return arrow: Task completion → Task Pool
    ax.annotate('', xy=(2, 5.8), xytext=(11.25, 5.8),
               arrowprops=dict(arrowstyle='->', color='#95a5a6', lw=1.5, linestyle='--',
                              connectionstyle='arc3,rad=0.5'))
    ax.text(6.7, 7.9, "任务完成回收", fontsize=9, color='#95a5a6', ha='center')

    # Outer border
    border = plt.Rectangle((0, 1), 14, 8.5, linewidth=2.5, edgecolor='#2c3e50',
                          facecolor='none', linestyle='-')
    ax.add_patch(border)

    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "vm_architecture.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════
# 图12: 配置方案对比
# ═══════════════════════════════════════════════════════════════

def chart_config_comparison(data=None):
    """绘制三种配置方案（克制/卑微/大方）对比"""
    fig, axes = plt.subplots(1, 3, figsize=(20, 7))

    configs = [
        {
            "title": "配置 A：克制型（默认推荐）",
            "subtitle": "cpu=75%, memory=50%",
            "color": "#27ae60",
            "params": {
                "cpu_limit": ("1.5 核", "2 × 0.75"),
                "mem_limit": ("700 MB", "1400 × 0.50"),
                "C": ("4", "1.5×0.75/0.25"),
                "M": ("21", "700×0.50/16"),
                "H": ("4", "min(4, 21, ...)"),
                "N_batch": "4",
                "每任务": "~175 MB",
            }
        },
        {
            "title": "配置 B：卑微型",
            "subtitle": "cpu=25%, memory=128MB",
            "color": "#e74c3c",
            "params": {
                "cpu_limit": ("0.5 核", "2 × 0.25"),
                "mem_limit": ("128 MB", "绝对值"),
                "C": ("1", "0.5×0.75/0.25"),
                "M": ("4", "128×0.50/16"),
                "H": ("1", "min(1, 4, ...)"),
                "N_batch": "1",
                "每任务": "~128 MB",
            }
        },
        {
            "title": "配置 C：大方型（独占整机）",
            "subtitle": "cpu=100%, memory=80%",
            "color": "#2980b9",
            "params": {
                "cpu_limit": ("2.0 核", "2 × 1.00"),
                "mem_limit": ("1120 MB", "1400 × 0.80"),
                "C": ("6", "2.0×0.75/0.25"),
                "M": ("35", "1120×0.50/16"),
                "H": ("6", "min(6, 35, ...)"),
                "N_batch": "6",
                "每任务": "~186 MB",
            }
        },
    ]

    for idx, cfg in enumerate(configs):
        ax = axes[idx]
        ax.set_xlim(0, 3)
        ax.set_ylim(0, 8)

        # Title
        ax.text(1.5, 7.6, cfg["title"], ha='center', fontsize=12, fontweight='bold', color=cfg["color"])
        ax.text(1.5, 7.1, cfg["subtitle"], ha='center', fontsize=9, color='#666666', style='italic')

        # Draw parameter rows
        y_positions = [6.3, 5.5, 4.7, 3.9, 3.1, 2.3, 1.5]
        param_keys = ["cpu_limit", "mem_limit", "C", "M", "H", "N_batch", "每任务"]
        labels = ["cpu_limit", "mem_limit", "C = CPU 上限", "M = MEM 上限",
                  "H = 硬上限", "N_batch", "每任务内存"]

        for i, (key, label) in enumerate(zip(param_keys, labels)):
            y = y_positions[i]
            val = cfg["params"][key]
            if isinstance(val, tuple):
                display_val, formula = val
                ax.text(1.5, y, f'{label}:  {display_val}', ha='center', fontsize=10,
                       fontweight='bold', color='#333333')
                ax.text(1.5, y - 0.25, f'({formula})', ha='center', fontsize=8, color='#888888')
            else:
                ax.text(1.5, y, f'{label}:  {val}', ha='center', fontsize=10,
                       fontweight='bold', color='#333333')

        # Highlight bar for N_batch
        n_batch_val = cfg["params"]["N_batch"]
        nb_y = 2.3
        bar_w = float(n_batch_val) * 0.25
        rect = plt.Rectangle((1.5 - bar_w/2, nb_y - 0.35), bar_w, 0.5, linewidth=0,
                            facecolor=cfg["color"], alpha=0.2)
        ax.add_patch(rect)

        ax.axis('off')

    fig.suptitle("同一台 2C2G 服务器，不同配置的并发效果\n（越克制 → 越卑微 → N_batch 越小）",
                fontsize=15, fontweight='bold')
    plt.tight_layout()
    path = os.path.join(ASSETS_DIR, "config_comparison.png")
    plt.savefig(path, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"  已保存: {path}")
    return path


# ═══════════════════════════════════════════════════════════════

def main():
    print(f"使用字体: {_CN_FONT}")
    print("正在生成全中文科学图表...\n")

    # 尝试加载仿真数据（如果存在）
    data = None
    master_path = os.path.join(REPORTS_DIR, "master_summary.json")
    if os.path.exists(master_path):
        data = load_all_data()
        print("已加载仿真数据，将生成数据驱动图表\n")
    else:
        print("未找到仿真数据，跳过数据驱动图表\n")

    # ── 数据驱动图表（需要仿真数据）──
    if data is not None:
        chart_heatmap(data)
        chart_pareto(data)
        chart_oom_comparison(data)
        chart_improvement_radar(data)

    # ── 分析型图表（使用合成数据，不需要仿真）──
    chart_factor_behavior(data)
    chart_oom_feedback_comparison(data)
    chart_slipway_viz(data)
    chart_merge_comparison(data)

    # ── 文档插图（纯示意，不需要仿真数据）──
    chart_instruction_encodings()
    chart_memory_watermark()
    chart_vm_architecture()
    chart_config_comparison()

    total = 8 + 4  # 原有 8 张 + 新增 4 张（指令编码是 1 张合图）
    print(f"\n全部 {total} 张图表已保存至 {ASSETS_DIR}/")


if __name__ == "__main__":
    main()
