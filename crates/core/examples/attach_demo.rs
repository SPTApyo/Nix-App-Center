//! Verification sandbox: attach + generation on a COPY of a real config.
//! Driven by `ONYX_CONFIG_DIR` / `XDG_DATA_HOME`; never touches the real system.

use onyx_core::{AttachPlan, Package};

fn main() {
    let l = onyx_core::detect_layout();
    println!("== layout ==");
    println!("{:?}\nhm_user : {:?}\napply   : {}\n", l.kind, l.hm_user, l.apply.join(" "));

    match onyx_core::attach_plan(&l) {
        AttachPlan::Patch { ref diff, .. } => {
            println!("== attach diff ==\n{diff}");
            onyx_core::attach_commit(&onyx_core::attach_plan(&l)).expect("commit");
            println!("attached = {}\n", onyx_core::is_attached(&l));
        }
        AttachPlan::AlreadyAttached => println!("already attached\n"),
        other => println!("plan : {other:?}\n"),
    }

    // One plain app, one promoted app (the "advanced path").
    onyx_core::keep(&Package::from_attr("ripgrep")).expect("keep");
    onyx_core::keep(&Package::from_attr("firefox")).expect("keep");
    let quickshare = Package::from_attr("quickshare");
    onyx_core::set_blocks(
        &quickshare,
        Some(format!(
            "{}\nservices.quickshare = {{\n  enable = true;\n  port = 8080;\n}};",
            onyx_core::default_home_block(&l, "quickshare")
        )),
        Some("networking.firewall.allowedTCPPorts = [ 8080 ];".into()),
    )
    .expect("set_blocks");

    for scope in [l.home.as_ref(), l.system.as_ref()].into_iter().flatten() {
        let body = std::fs::read_to_string(&scope.onyx_file).expect("generated file");
        println!("== {} ==\n{body}", scope.onyx_file.display());
    }
}
