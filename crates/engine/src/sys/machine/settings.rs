//! 影响开发的几个 Windows 设置，以及从它们能看出什么。
//!
//! 只挑会影响开发的那几项——每一项都能直接改变「编译快不快、会不会莫名失败」。

use super::{reg, Fact};

pub fn system_settings() -> Vec<Fact> {
    let mut out = Vec::new();

    // 电源计划：对满载编译的影响最大的一项
    if let Some(g) = reg(
        r"HKLM\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes",
        "ActivePowerScheme",
    ) {
        // 把「现在靠什么供电」一起写进去：离开这一句，电源计划是好是坏根本没法判断
        let supply = match super::ffi::on_ac_power() {
            Some(true) => " · 插着电",
            Some(false) => " · 用电池",
            None => "",
        };
        out.push(Fact::one(
            "电源计划",
            format!("{}{}", power_plan_name(&g), supply),
            "注册表",
        ));
    }

    // 虚拟内存：关掉它，编译到内存峰值会被系统直接结束进程
    let pf = reg(
        r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management",
        "PagingFiles",
    )
    .unwrap_or_default();
    out.push(Fact::one(
        "虚拟内存",
        if pf.trim().is_empty() {
            "已关闭".to_string()
        } else {
            format!("已启用（{}）", pf.trim())
        },
        "注册表",
    ));

    // 长路径：不开的话深层 node_modules 会撞上 260 字符上限
    let lp = reg(
        r"HKLM\SYSTEM\CurrentControlSet\Control\FileSystem",
        "LongPathsEnabled",
    )
    .unwrap_or_default();
    out.push(Fact::one(
        "长路径支持",
        if lp.eq_ignore_ascii_case("0x1") {
            "已开启".to_string()
        } else {
            "未开启".to_string()
        },
        "注册表",
    ));

    // 内存完整性（核心隔离）：给每个内核调用加一层校验
    let hv = reg(
        r"HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity",
        "Enabled",
    )
    .unwrap_or_default();
    out.push(Fact::one(
        "内存完整性",
        if hv.eq_ignore_ascii_case("0x1") {
            "已开启".to_string()
        } else {
            "已关闭".to_string()
        },
        "注册表",
    ));
    out
}

/// 电源计划的 GUID → 名字。这几个是 Windows 自带的固定方案。
fn power_plan_name(guid: &str) -> String {
    let g = guid.trim().to_lowercase();
    let name = match g.as_str() {
        "381b4222-f694-41f0-9685-ff5bb260df2e" => "平衡",
        "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c" => "高性能",
        "a1841308-3541-4fab-bc81-f71556f20b4a" => "节能",
        "e9a42b02-d5df-448d-aa00-03f14749eb61" => "卓越性能",
        _ => "自定义方案",
    };
    format!("{}（{}）", name, g)
}

/// 从这几个设置能看出什么。仍然只讲能证实的。
pub fn setting_judgements(facts: &[Fact]) -> Vec<String> {
    let mut out = Vec::new();
    let val = |label: &str| -> String {
        facts
            .iter()
            .find(|f| f.label == label)
            .map(|f| f.value.clone())
            .unwrap_or_default()
    };
    // 只有插着电才提电源计划。用电池时本来就该省电。
    let plan = val("电源计划");
    let on_ac = super::ffi::on_ac_power();
    if !plan.is_empty()
        && !plan.starts_with("高性能")
        && !plan.starts_with("卓越性能")
        && on_ac != Some(false)
    {
        out.push(
            "当前电源计划不是「高性能」：插电满载时 CPU 会被限频，编译能差两三成。控制面板 → 电源选项 → 选「高性能」".into(),
        );
    }
    if val("虚拟内存").starts_with("已关闭") {
        out.push(
            "虚拟内存被关了。编译这类内存峰值高的活，峰值一超就会被系统直接结束进程，报错往往看不出原因".into(),
        );
    }
    if val("长路径支持").starts_with("未开启") {
        out.push(
            "长路径支持没开：路径超过 260 字符就写不进去，深层 node_modules 和嵌套很深的 Java 包名最先撞上".into(),
        );
    }
    if val("内存完整性").starts_with("已开启") {
        out.push(
            "内存完整性开着：它给每个内核调用加一层校验，实测拖慢一到两成，而它防的是驱动级攻击，跟开发环境无关。要关：Windows 安全中心 → 设备安全性 → 内核隔离 → 内存完整性。代价是少了那层保护".into(),
        );
    }
    out
}


