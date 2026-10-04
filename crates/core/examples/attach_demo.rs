//! Bac à sable de vérification : greffe + génération sur une COPIE d'une vraie config.
//! Piloté par `ONYX_CONFIG_DIR` / `XDG_DATA_HOME` — ne touche jamais au vrai système.

use onyx_core::{AttachPlan, Package};

fn main() {
    let l = onyx_core::detect_layout();
    println!("== disposition ==");
    println!("{:?}\nhm_user : {:?}\napply   : {}\n", l.kind, l.hm_user, l.apply.join(" "));

    match onyx_core::attach_plan(&l) {
        AttachPlan::Patch { ref diff, .. } => {
            println!("== diff de greffe ==\n{diff}");
            onyx_core::attach_commit(&onyx_core::attach_plan(&l)).expect("commit");
            println!("greffé. attaché = {}\n", onyx_core::is_attached(&l));
        }
        AttachPlan::AlreadyAttached => println!("déjà greffé\n"),
        other => println!("plan : {other:?}\n"),
    }

    // Une app simple, une app promue (le « chemin avancé »).
    onyx_core::keep(&Package::from_attr("ripgrep")).expect("keep");
    onyx_core::keep(&Package::from_attr("firefox")).expect("keep");
    let quickshare = Package::from_attr("quickshare");
    onyx_core::set_blocks(
        &quickshare,
        Some(format!(
            "{}\nservices.quickshare = {{\n  enable = true;\n  port = 8080;\n}};",
            onyx_core::default_home_block("quickshare")
        )),
        Some("networking.firewall.allowedTCPPorts = [ 8080 ];".into()),
    )
    .expect("set_blocks");

    for scope in [l.home.as_ref(), l.system.as_ref()].into_iter().flatten() {
        let body = std::fs::read_to_string(&scope.onyx_file).expect("fichier généré");
        println!("== {} ==\n{body}", scope.onyx_file.display());
    }
}
