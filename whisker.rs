// `whisker.rs` — Whisker app configuration.
//
// `whisker run` compiles this file as a tiny probe binary that
// serializes the resulting `Config` to JSON; the CLI reads that
// JSON and projects it into the dev-server's flat `Config`.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("Whisker Drawapp")
        .bundle_id("rs.example.whisker_drawapp")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.example.whisker_drawapp")
            .application_id("rs.example.whisker_drawapp")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.example.whisker_drawapp")
            .scheme("Whisker Drawapp")
            .deployment_target("13.0");
    });
}
