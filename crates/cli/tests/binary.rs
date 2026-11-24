use std::process::Command;

#[test]
fn the_binary_runs() {
    let out = Command::new(env!("CARGO_BIN_EXE_klad"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("klad"));
}

#[test]
fn skill_prints_the_klad_skill_and_the_drawing_and_pixel_skills() {
    let skill = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .arg("skill")
            .args(args)
            .output()
            .unwrap()
    };
    let said = |args: &[&str]| String::from_utf8_lossy(&skill(args).stdout).to_string();
    assert!(said(&[]).starts_with("---\nname: klad\n"));
    assert_eq!(said(&["klad"]), said(&[]));
    let drawing = said(&["drawing"]);
    assert!(drawing.starts_with("---\nname: klad-drawing\n"));
    assert!(drawing.contains("## The order of work"));
    let pixel = said(&["pixel"]);
    assert!(pixel.starts_with("---\nname: klad-pixel\n"));
    assert!(pixel.contains("## Clusters") && pixel.contains("## Before you show it"));
    assert!(said(&[]).contains("klad skill pixel"), "the klad skill points to it");
    let reference = String::from_utf8_lossy(
        &Command::new(env!("CARGO_BIN_EXE_klad"))
            .arg("reference")
            .output()
            .unwrap()
            .stdout,
    )
    .to_string();
    let words = |text: &str, lead: &str| -> std::collections::BTreeSet<String> {
        text.match_indices(lead)
            .map(|(at, _)| {
                text[at + lead.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect::<String>()
            })
            .filter(|word| word.starts_with(|c: char| c.is_ascii_lowercase()))
            .map(|word| word.trim_end_matches('-').to_string())
            .collect()
    };
    let commands = words(&pixel, "`klad ");
    assert!(commands.len() >= 8, "{commands:?}");
    commands.iter().for_each(|command| {
        assert!(
            reference.contains(&format!("## klad {command}")) || reference.contains(&format!("klad {command} ")),
            "the pixel skill names klad {command}, which klad does not have"
        );
    });
    let flags = words(&pixel, "--");
    assert!(flags.len() >= 12, "{flags:?}");
    flags.iter().for_each(|flag| {
        assert!(
            reference.contains(&format!("--{flag}")),
            "the pixel skill names --{flag}, which no command has"
        );
    });
    let unknown = skill(&["painting"]);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no skill named painting"));
}

#[test]
fn a_drawing_moves_and_a_layer_duplicates_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-move-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let path = bundle.to_str().unwrap();
    draw(&["new", path, "--width", "200", "--height", "100", "--frames", "12"]);
    draw(&["stroke", path, "l1", "--path", "M10 10 L190 90", "--size", "4"]);
    draw(&["cel", path, "dup", "c2", "--frame", "8", "--hold", "2"]);
    draw(&["cel", path, "move", "c2", "--frame", "2"]);
    assert!(draw(&["duplicate", path, "paint 1"]).contains("added"));
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(bundle.join("doc.json")).unwrap()).unwrap();
    let layers = doc["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1]["name"], "paint 1 copy");
    let starts = |l: &serde_json::Value| -> Vec<u64> {
        l["cels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["frame"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(starts(&layers[0]), [2, 10]);
    assert_eq!(starts(&layers[1]), [2, 10]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_error_is_one_line_and_the_backtrace_waits_for_klad_backtrace() {
    let missing = std::env::temp_dir().join(format!("draw-binary-missing-{}.klad", std::process::id()));
    let run = |backtrace: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_klad"));
        command
            .args(["ls", missing.to_str().unwrap()])
            .env("RUST_BACKTRACE", "1");
        match backtrace {
            Some(v) => command.env("KLAD_BACKTRACE", v),
            None => command.env_remove("KLAD_BACKTRACE"),
        };
        let out = command.output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let plain = run(None);
    assert_eq!(plain.lines().count(), 1, "{plain}");
    assert!(
        plain.starts_with("error: reading ") && plain.contains("doc.json: "),
        "{plain}"
    );
    assert_eq!(run(Some("0")), plain);
    let traced = run(Some("1"));
    assert!(traced.contains("Caused by") && traced.contains("backtrace"), "{traced}");
}

#[test]
fn the_client_makes_edits_reads_and_renders_a_drawing() {
    use klad_client::{Error, Klad, json};
    let dir = std::env::temp_dir().join(format!("draw-binary-client-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let klad = Klad::at(env!("CARGO_BIN_EXE_klad"));
    assert!(klad.version().unwrap().starts_with("klad"));
    let art = klad.drawing(dir.join("art.klad"));
    art.create(320, 200, Some("#e9e4da")).unwrap();
    let ids = art
        .apply([
            json!({"op": "add", "layer": {"kind": "vector", "name": "sky"}}),
            json!({"op": "stroke", "layer": "l1", "brush": "ink", "size": 6, "color": "#1b1b1b", "path": "M20 100 L300 100"}),
        ])
        .unwrap();
    assert!(!ids.is_empty());
    let layer = art.document().unwrap()["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "sky")
        .map(|l| l["id"].as_str().unwrap().to_string())
        .unwrap();
    art.apply([json!({"op": "add", "node": {"type": "rect", "x": 10, "y": 10, "w": 50, "h": 30, "fill": "#336699"}, "in": layer, "frame": 0})])
        .unwrap();
    assert!(art.ls(0).unwrap().to_string().contains("sky"));
    let strokes = art.strokes("l1", 0).unwrap();
    assert!(strokes.to_string().contains("ink"));
    let png = art.render(&dir.join("art.png"), 0, 0.5).unwrap();
    assert!(std::fs::metadata(png).unwrap().len() > 0);
    art.undo().unwrap();
    assert!(!art.document().unwrap().to_string().contains("#336699"));
    let missing = art.show("n999").unwrap_err();
    assert!(matches!(missing, Error::Failed { .. }), "{missing}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn updates_says_what_the_feed_holds() {
    let dir = std::env::temp_dir().join(format!("draw-binary-updates-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let feed = dir.join("latest.json");
    let run = |feed: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .arg("updates")
            .env("KLAD_UPDATE_FEED", feed)
            .output()
            .unwrap()
    };
    std::fs::write(
        &feed,
        r#"{"tag_name":"v9.9.9","html_url":"https://github.com/klad-app/klad/releases","body":"- a test"}"#,
    )
    .unwrap();
    let out = run(&feed);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "klad 9.9.9 is out: a test\nhttps://github.com/klad-app/klad/releases\n"
    );
    std::fs::write(&feed, r#"{"tag_name":"v0.0.1","html_url":"https://example.org"}"#).unwrap();
    assert!(String::from_utf8_lossy(&run(&feed).stdout).contains("is the newest version"));
    let missing = run(&dir.join("missing.json"));
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).starts_with("error: could not check: reading"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn samples_are_listed_and_copied_to_draw_on() {
    let dir = std::env::temp_dir().join(format!("draw-binary-samples-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let shipped = dir.join("shipped");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .env("KLAD_SAMPLES", &shipped)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    std::fs::create_dir_all(&shipped).unwrap();
    let rock = shipped.join("rock/rock.klad");
    let out = run(&["new", rock.to_str().unwrap(), "--width", "64", "--height", "64"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::write(
        shipped.join("shipped.json"),
        r#"[{"name":"rock","bundle":"rock/rock.klad","about":"a rock on its own"}]"#,
    )
    .unwrap();
    let listed = String::from_utf8_lossy(&run(&["samples"]).stdout).to_string();
    assert!(listed.starts_with("rock       a rock on its own"), "{listed}");
    let copied = run(&["samples", "--copy", "rock"]);
    assert!(copied.status.success(), "{}", String::from_utf8_lossy(&copied.stderr));
    assert!(
        dir.join("rock.klad/doc.json").is_file(),
        "the copy goes in the current folder"
    );
    let into = dir.join("mine");
    let out = run(&["samples", "--copy", "rock", "--into", into.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("copied rock to {}", into.join("rock.klad").display())
    );
    let wrong = run(&["samples", "--copy", "stone"]);
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("no sample stone; there are rock"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_report_holds_the_version_the_log_with_home_hidden_and_zips_the_drawing() {
    let dir = std::env::temp_dir().join(format!("draw-binary-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let home = dir.join("home");
    let log = dir.join("klad.log");
    std::fs::write(
        &log,
        format!("2026-11-06 17:00:00 editor: could not read {}/x.png\n", home.display()),
    )
    .unwrap();
    let bundle = dir.join("drawings/rock.klad");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .env("KLAD_LOG", &log)
            .env("HOME", &home)
            .output()
            .unwrap()
    };
    assert!(
        run(&["new", bundle.to_str().unwrap(), "--width", "64", "--height", "48"])
            .status
            .success()
    );
    let out = run(&["report", "--drawing", bundle.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        text.starts_with("klad ") && text.contains("from the command line"),
        "{text}"
    );
    assert!(
        text.contains("could not read ~/x.png"),
        "the home folder reads as ~: {text}"
    );
    assert!(
        text.contains("The drawing (64 by 48 px, 1 layer, 1 frame) is attached as rock for a report.zip."),
        "{text}"
    );
    assert!(dir.join("drawings/rock for a report.zip").is_file());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn solo_shows_one_layer_and_undo_brings_the_rest_back() {
    let dir = std::env::temp_dir().join(format!("draw-binary-solo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    run(&["new", path, "--width", "64", "--height", "64", "--layers", "3"]);
    let shown = || -> Vec<bool> {
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(bundle.join("doc.json")).unwrap()).unwrap();
        doc["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["visible"].as_bool().unwrap_or(true))
            .collect()
    };
    run(&["solo", path, "paint 2"]);
    assert_eq!(shown(), [false, true, false]);
    assert!(run(&["solo", path, "paint 2"]).contains("already shows alone"));
    run(&["undo", path]);
    assert_eq!(shown(), [true, true, true]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_chain_and_its_ik_are_set_up_and_posed_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("klad-binary-ik-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let klad = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let path = bundle.to_str().unwrap();
    klad(&["new", path, "--width", "400", "--height", "400", "--frames", "6"]);
    klad(&[
        "stroke",
        path,
        "l1",
        "--path",
        "M100 100 L100 200 L100 300",
        "--size",
        "6",
    ]);
    klad(&["add", path, "layer", "bones", "name=bones"]);
    let made = klad(&[
        "bone",
        path,
        "bones",
        "--chain",
        "thigh,shin",
        "--through",
        "100,100 100,200 100,300",
    ]);
    assert!(made.contains("chain thigh, shin"), "{made}");
    let rigged = klad(&["attach", path, "l1", "--bend", "--skeleton", "bones"]);
    assert!(rigged.contains("bends with bones"), "{rigged}");
    let ik = klad(&["ik", path, "bones", "--add", "shin", "--softness", "4"]);
    assert!(ik.contains("a limb with thigh") && ik.contains("softness 4"), "{ik}");
    klad(&["ik", path, "bones", "--set", "shin", "--bend", "flip"]);
    klad(&[
        "pose",
        path,
        "bones",
        "--from",
        "0",
        "--to",
        "3",
        "--target",
        "shin=180,240",
        "--mix",
        "shin=0.5",
        "--root",
        "-10,0",
    ]);
    let listed = klad(&["bones", path]);
    assert!(
        listed.contains("ik shin: a limb with thigh, bending counterclockwise, softness 4 px"),
        "{listed}"
    );
    assert!(
        listed.contains("root -10,0 target shin=180,240 mix 0.5 solved shin="),
        "{listed}"
    );
    let nan = Command::new(env!("CARGO_BIN_EXE_klad"))
        .args([
            "pose",
            path,
            "bones",
            "--from",
            "0",
            "--to",
            "4",
            "--target",
            "shin=nan,240",
        ])
        .output()
        .unwrap();
    assert!(!nan.status.success(), "a target that is not a number is refused");
    klad(&["ls", path]);
    let json: serde_json::Value = serde_json::from_str(&klad(&["bones", path, "--json"])).unwrap();
    assert_eq!(json[0]["ik"][0]["bone"], "shin");
    assert!(json[0]["poses"][0]["solved"]["thigh"].is_number());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_checkout_under_the_cache_renders_the_drawing_it_brings() {
    let dir = std::env::temp_dir().join(format!("draw-binary-checkout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let render = || {
        let printed = draw(&["render", path]);
        std::fs::read(printed.lines().last().unwrap().trim()).unwrap()
    };
    let files = || ["doc.json", "strokes/l1/c2.strokes"].map(|f| std::fs::read(bundle.join(f)).ok());
    let put = |kept: &[Option<Vec<u8>>; 2]| {
        ["doc.json", "strokes/l1/c2.strokes"]
            .iter()
            .zip(kept)
            .for_each(|(f, bytes)| match bytes {
                Some(bytes) => std::fs::write(bundle.join(f), bytes).unwrap(),
                None => {
                    let _ = std::fs::remove_file(bundle.join(f));
                }
            });
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    let base = files();
    draw(&["stroke", path, "l1", "--path", "M10 10 L190 90", "--size", "8"]);
    let one = files();
    let first = render();
    put(&base);
    draw(&["stroke", path, "l1", "--path", "M10 90 L190 10", "--size", "8"]);
    let other = render();
    assert_ne!(
        first, other,
        "the other branch, after as many edits, is not the first one's render"
    );
    put(&one);
    assert_eq!(
        render(),
        first,
        "checking the first branch out again renders it, not the cached other"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_merge_that_left_an_id_used_twice_is_repaired() {
    let dir = std::env::temp_dir().join(format!("draw-binary-repair-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100", "--layers", "2"]);
    draw(&["stroke", path, "l1", "--path", "M10 10 L190 90"]);
    draw(&["stroke", path, "l3", "--path", "M10 90 L190 10"]);
    let doc = std::fs::read_to_string(bundle.join("doc.json")).unwrap();
    std::fs::write(bundle.join("doc.json"), doc.replace("\"l3\"", "\"l1\"")).unwrap();
    std::fs::rename(
        bundle.join("strokes/l3/c4.strokes"),
        bundle.join("strokes/l1/c4.strokes"),
    )
    .unwrap();
    let refused = run(&["info", path]);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        !refused.status.success() && said.contains("id l1 is used twice") && said.contains("klad repair"),
        "{said}"
    );
    let repaired = draw(&["repair", path]);
    assert!(repaired.contains("layer \"paint 2\": l1 is now l5"), "{repaired}");
    assert!(
        repaired.contains("moved strokes/l1/c4.strokes to strokes/l5/c4.strokes"),
        "{repaired}"
    );
    let info = draw(&["info", path]);
    assert!(
        info.contains("l1      c2") && info.contains("l5      c4") && info.matches("1 strokes").count() == 2,
        "{info}"
    );
    assert!(draw(&["repair", path]).contains("nothing to repair"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn shapes_align_spread_and_centre_on_the_canvas_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-align-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "400", "--height", "200", "--sketch"]);
    let add = |x: &str, y: &str, w: &str| {
        let made = draw(&[
            "add",
            path,
            "rect",
            "--in",
            "l3",
            &format!("x={x}"),
            &format!("y={y}"),
            &format!("w={w}"),
            "h=20",
        ]);
        made.lines()
            .find_map(|line| line.strip_prefix("added "))
            .unwrap_or_else(|| panic!("an id in: {made}"))
            .to_string()
    };
    let (a, b, c) = (add("10", "10", "20"), add("100", "40", "40"), add("50", "80", "10"));
    let boxes = || {
        [&a, &b, &c].map(|id| {
            let node: serde_json::Value = serde_json::from_str(&draw(&["show", path, id])).unwrap();
            [0, 1].map(|i| node["bounds"][i].as_f64().unwrap())
        })
    };
    draw(&["align", path, &a, &b, &c, "--y", "top", "--spread", "x"]);
    assert_eq!(boxes(), [[10.0, 10.0], [100.0, 10.0], [60.0, 10.0]]);
    draw(&["align", path, &b, "--x", "centre", "--y", "middle", "--to", "canvas"]);
    assert_eq!(boxes()[1], [180.0, 90.0]);
    draw(&["align", path, &b, &c, "--x", "left", "--to", &a]);
    assert_eq!(boxes().map(|at| at[0]), [10.0, 10.0, 10.0]);
    assert!(draw(&["align", path, &b, &c, "--x", "left", "--to", &a]).contains("nothing moved"));
    let log = draw(&["log", path]);
    assert_eq!(
        log.lines().filter(|line| line.contains("set n")).count(),
        3,
        "one history step each: {log}"
    );
    let refused = run(&["align", path, &a, "--x", "left"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("nothing to line up with"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_pixel_drawing_is_made_drawn_listed_and_edited_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-pixel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let json = |args: &[&str]| -> serde_json::Value { serde_json::from_str(&draw(args)).unwrap() };
    draw(&["new", path, "--pixel", "16x12"]);
    let tree = json(&["ls", path, "--json"]);
    assert_eq!(
        (tree["canvas"]["width"].as_u64(), tree["canvas"]["height"].as_u64()),
        (Some(16), Some(12))
    );
    let layer = tree["layers"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(tree["layers"][0]["kind"], "pixel");
    draw(&["palette", path, "--add", "#f0c8a0", "--name", "skin"]);
    let made = draw(&[
        "pixel",
        path,
        &layer,
        "--color",
        "#2b2d42",
        "--cells",
        "3-12,2 3,3-8 12,3-8 3-12,9",
    ]);
    assert!(
        made.contains("pixel marks") && made.contains("changed palette"),
        "{made}"
    );
    draw(&["pixel", path, &layer, "--color", "skin", "--cells", "4-11,3-8"]);
    draw(&["pixel", path, &layer, "--erase", "--cells", "7,5 8,5"]);
    let listed = json(&["strokes", path, &layer, "--json"]);
    let marks = listed["fills"].as_array().unwrap();
    assert_eq!(marks.len(), 2, "the eraser leaves no mark of its own");
    assert_eq!(marks[0]["cells"].as_array().unwrap().len(), 32);
    assert_eq!(
        marks[1]["cells"].as_array().unwrap().len(),
        46,
        "it takes its two pixels out of the mark under it"
    );
    let palette = json(&["palette", path, "--json"]);
    assert_eq!(palette.as_array().unwrap().len(), 2);
    assert_eq!(
        marks[1]["entry"], palette[0]["id"],
        "the mark is bound to the skin entry"
    );
    assert_eq!(marks[0]["entry"], palette[1]["id"]);
    let text = draw(&["strokes", path, &layer]);
    assert!(
        text.contains("2 pixel marks, 78 cells") && text.contains("skin") && !text.contains("erases"),
        "{text}"
    );
    let first = marks[0]["address"].as_str().unwrap().to_string();
    draw(&["set", path, &first, "color=skin"]);
    draw(&["transform", path, &first, "--move", "1,0"]);
    let after = json(&["strokes", path, &layer, "--json"]);
    assert_eq!(after["fills"][0]["entry"], palette[0]["id"]);
    assert_eq!(after["fills"][0]["bounds"][0].as_f64(), Some(4.0));
    assert!(draw(&["ls", path]).contains("2 pixel marks"));
    let refused = run(&["stroke", path, &layer, "--path", "M1 1 L9 9"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("pixel layer"));
    let bad = run(&["pixel", path, &layer, "--color", "skin", "--cells", "3;4"]);
    assert!(String::from_utf8_lossy(&bad.stderr).contains("is not x,y"));
    let added = draw(&["add", path, "layer", "pixel", "name=lines"]);
    let lines_id = added
        .lines()
        .find_map(|line| line.strip_prefix("added "))
        .unwrap_or_else(|| panic!("an id in: {added}"))
        .to_string();
    draw(&[
        "pixel",
        path,
        "lines",
        "--color",
        "skin",
        "--path",
        "M0.5 11.5 C 4 2 11 2 15.5 11.5",
    ]);
    draw(&[
        "pixel",
        path,
        &lines_id,
        "--color",
        "skin",
        "--path",
        "M1.5 1.5 L5.5 1.5 L5.5 4.5",
    ]);
    let lines = json(&["strokes", path, &lines_id, "--json"]);
    let cells = |at: usize| -> Vec<(i64, i64)> {
        lines["fills"][at]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c[0].as_i64().unwrap(), c[1].as_i64().unwrap()))
            .collect()
    };
    let curve = cells(0);
    assert!(curve.contains(&(0, 11)) && curve.contains(&(15, 11)), "{curve:?}");
    let doubled = curve.iter().any(|&(x, y)| {
        let has = |dx: i64, dy: i64| curve.contains(&(x + dx, y + dy));
        (has(1, 0) || has(-1, 0))
            && (has(0, 1) || has(0, -1))
            && !(has(1, 0) && has(-1, 0))
            && !(has(0, 1) && has(0, -1))
    });
    assert!(!doubled, "a curve is one pixel thin with no corner doubled: {curve:?}");
    assert_eq!(cells(1).len(), 8, "a path's own corner is kept");
    draw(&["pixel", path, &lines_id, "--erase", "--size", "3", "--cells", "5,1"]);
    let erased = json(&["strokes", path, &lines_id, "--json"]);
    assert_eq!(
        (
            erased["fills"].as_array().unwrap().len(),
            erased["fills"][1]["cells"].as_array().unwrap().len()
        ),
        (2, 5),
        "a size is a square of whole pixels: the eraser takes three of the path's and leaves no mark"
    );
    draw(&["rm", path, &lines_id]);
    let wide = run(&[
        "pixel", path, &layer, "--color", "skin", "--cells", "1,1", "--size", "99",
    ]);
    assert!(String::from_utf8_lossy(&wide.stderr).contains("from 1 to 64"));
    let shown = run(&["render", path]);
    assert!(
        String::from_utf8_lossy(&shown.stderr).contains("512x384"),
        "a pixel drawing renders enlarged by a whole number: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let hash = |out: &std::process::Output| {
        String::from_utf8_lossy(&out.stderr)
            .split_whitespace()
            .skip_while(|word| *word != "hash")
            .nth(1)
            .map(String::from)
    };
    let gridded = run(&["render", path, "--overlay", "grid"]);
    assert!(
        gridded.status.success() && hash(&gridded) != hash(&shown),
        "the grid overlay draws its lines"
    );
    let real = run(&["render", path, "--scale", "1"]);
    assert!(String::from_utf8_lossy(&real.stderr).contains("16x12"));
    draw(&["add", path, "layer", "pixel", "name=shade"]);
    assert_eq!(json(&["ls", path, "--json"])["layers"][0]["kind"], "pixel");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_palette_recolours_its_pixels_loads_palettes_and_groups_ramps_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-palette-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let json = |args: &[&str]| -> serde_json::Value { serde_json::from_str(&draw(args)).unwrap() };
    draw(&["new", path, "--pixel", "8"]);
    draw(&["palette", path, "--add", "#f0c8a0", "--name", "skin"]);
    draw(&["pixel", path, "l1", "--color", "skin", "--cells", "1-3,1-3"]);
    draw(&["palette", path, "--recolour", "skin", "--to", "#e8b796"]);
    let marks = json(&["strokes", path, "l1", "--json"]);
    assert_eq!(
        marks["fills"][0]["color"], "#e8b796ff",
        "the pixels follow their palette colour"
    );
    draw(&["palette", path, "--ramp", "skin", "--base", "#e8b796", "--steps", "3"]);
    let palette = json(&["palette", path, "--json"]);
    let ramp: Vec<(&str, &str)> = palette
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap_or(""), s["ramp"].as_str().unwrap_or("")))
        .collect();
    assert_eq!(
        ramp,
        [("skin-1", "skin"), ("skin", "skin"), ("skin-3", "skin")],
        "the colour the palette has takes its place in the middle of its ramp"
    );
    assert!(draw(&["palettes"]).contains("pico-8"));
    assert_eq!(json(&["palettes", "--json"])[2]["colors"].as_array().unwrap().len(), 16);
    draw(&["palette", path, "--load", "four-greens"]);
    let file = dir.join("mine.hex");
    std::fs::write(&file, "081820\nabcdef\n").unwrap();
    draw(&["palette", path, "--load", file.to_str().unwrap()]);
    assert_eq!(
        json(&["palette", path, "--json"]).as_array().unwrap().len(),
        8,
        "three of the ramp, the four greens, one new from the file"
    );
    draw(&["palette", path, "--ramp", "greens", "--of", "#081820,#346856,#88c070"]);
    assert!(draw(&["palette", path]).contains("ramp greens"));
    let refused = run(&["palette", path, "--load", "nope"]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("comes with klad"));
    let lonely = run(&["palette", path, "--ramp", "x"]);
    assert!(String::from_utf8_lossy(&lonely.stderr).contains("--base"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sprite_is_written_as_a_grid_of_characters_and_read_back_the_same() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = std::env::temp_dir().join(format!("draw-binary-grid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--pixel", "8x6"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    let text = "i = ink\nb = #f2c14e\n\n..iiii..\n.ibbbbi.\n.ibbbbi.\n..iiii..\n";
    let mut child = Command::new(env!("CARGO_BIN_EXE_klad"))
        .args(["pixel", path, "l1", "--grid", "-", "--at", "0,1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let made = child.wait_with_output().unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    assert!(String::from_utf8_lossy(&made.stdout).contains("pixel marks"));
    let marks: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    assert_eq!(
        marks["fills"].as_array().unwrap().len(),
        2,
        "one mark for each colour of the grid"
    );
    let plain = draw(&["pixels", path, "l1", "--plain"]);
    assert_eq!(
        plain.trim_end(),
        ". = empty\ni = ink\na = #f2c14e\n\n........\n..iiii..\n.iaaaai.\n.iaaaai.\n..iiii..\n........"
    );
    let ruled = draw(&["pixels", path, "l1"]);
    assert!(ruled.contains("8x6") && ruled.contains(" 1  ..iiii.."), "{ruled}");
    let read: serde_json::Value = serde_json::from_str(&draw(&["pixels", path, "l1", "--json"])).unwrap();
    assert_eq!(read["rows"][2], ".iaaaai.");
    assert_eq!(read["legend"]["i"]["name"], "ink");
    assert_eq!((read["width"].as_u64(), read["height"].as_u64()), (Some(8), Some(6)));
    let copy = dir.join("copy.klad");
    let copy_path = copy.to_str().unwrap();
    draw(&["new", copy_path, "--pixel", "8x6"]);
    draw(&["palette", copy_path, "--add", "#22223b", "--name", "ink"]);
    let saved = dir.join("sprite.txt");
    std::fs::write(&saved, &plain).unwrap();
    draw(&["pixel", copy_path, "l1", "--grid", saved.to_str().unwrap()]);
    assert_eq!(
        draw(&["hash", copy_path]),
        draw(&["hash", path]),
        "what klad pixels prints draws the same picture again"
    );
    std::fs::write(&saved, "i = ink\n\n..q..\n").unwrap();
    let unknown = run(&["pixel", path, "l1", "--grid", saved.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("the legend does not name"));
    draw(&["add", path, "layer", "paint", "name=paint"]);
    let wrong = run(&["pixels", path, "paint"]);
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("reads pixel layers"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn lines_rectangles_ellipses_and_fills_go_on_the_grid_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-shapes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--pixel", "16x12"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["palette", path, "--add", "#f2c14e", "--name", "body"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--ellipse", "1,1,9,7"]);
    draw(&["pixel", path, "l1", "--color", "body", "--fill", "5,4"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--rect", "11,1,4,4"]);
    draw(&["pixel", path, "l1", "--color", "body", "--rect", "11,6,4,3", "--filled"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--line", "0,11,15,9"]);
    let rows: Vec<String> = draw(&["pixels", path, "l1", "--plain"])
        .lines()
        .skip_while(|line| !line.is_empty())
        .skip(1)
        .map(String::from)
        .collect();
    assert_eq!(
        rows,
        [
            "................",
            "...iiiii...iiii.",
            "..ibbbbbi..i..i.",
            ".ibbbbbbbi.i..i.",
            ".ibbbbbbbi.iiii.",
            ".ibbbbbbbi......",
            "..ibbbbbi..bbbb.",
            "...iiiii...bbbb.",
            "...........bbbb.",
            "............iiii",
            "....iiiiiiii....",
            "iiii............",
        ]
    );
    draw(&["add", path, "layer", "pixel", "name=under", "--below", "l1"]);
    draw(&[
        "pixel", path, "under", "--color", "#8ecae6", "--fill", "12,2", "--ref", "l1",
    ]);
    let under: serde_json::Value = serde_json::from_str(&draw(&["pixels", path, "under", "--json"])).unwrap();
    assert_eq!(
        under["rows"][2], "............aa..",
        "a fill on another layer stops at the lines it reads"
    );
    assert_eq!(under["rows"][5], "................");
    let both = run(&[
        "pixel", path, "l1", "--color", "ink", "--rect", "0,0,2,2", "--line", "0,0,3,3",
    ]);
    assert!(!both.status.success());
    let flat = run(&["pixel", path, "l1", "--color", "ink", "--rect", "0,0,0,2"]);
    assert!(String::from_utf8_lossy(&flat.stderr).contains("at least 1 pixel"));
    let off = run(&["pixel", path, "l1", "--color", "ink", "--fill", "40,40"]);
    assert!(String::from_utf8_lossy(&off.stderr).contains("off the 16x12 canvas"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pixel_marks_are_listed_by_colour_cut_by_a_box_and_recoloured_exactly() {
    let dir = std::env::temp_dir().join(format!("draw-binary-pixel-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let rows = |layer: &str| -> Vec<String> {
        draw(&["pixels", path, layer, "--plain"])
            .lines()
            .skip_while(|line| !line.is_empty())
            .skip(1)
            .map(String::from)
            .collect()
    };
    draw(&["new", path, "--pixel", "12x6"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["palette", path, "--add", "#f2c14e", "--name", "body"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--rect", "0,0,6,4"]);
    draw(&["pixel", path, "l1", "--color", "body", "--rect", "1,1,4,2", "--filled"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--cells", "0,5"]);
    let ink: serde_json::Value =
        serde_json::from_str(&draw(&["strokes", path, "l1", "--entry", "ink", "--json"])).unwrap();
    assert_eq!(
        ink["fills"].as_array().unwrap().len(),
        2,
        "the two ink marks, not the body"
    );
    assert!(draw(&["strokes", path, "l1", "--entry", "body"]).contains("1 pixel marks, 8 cells"));
    let moved = draw(&["lasso", path, "pixel 1", "--box", "3,0,3,4", "--cut", "--move", "5,1"]);
    assert!(moved.contains("moved"), "{moved}");
    assert_eq!(
        rows("l1"),
        [
            "iii.........",
            "ibb.....iii.",
            "ibb.....bbi.",
            "iii.....bbi.",
            "........iii.",
            "i..........."
        ],
        "the right half of the box moved 5 across and 1 down, between whole pixels"
    );
    let marks: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    let pieces = marks["fills"].as_array().unwrap();
    assert_eq!(pieces.len(), 5, "the two marks the box crossed are four now");
    assert!(
        pieces.iter().all(|m| m.get("cuts").is_none()),
        "with no cut left on them"
    );
    let body_moved = pieces
        .iter()
        .find(|m| m["color"] == "#f2c14eff" && m["bounds"][0].as_f64() == Some(8.0))
        .map(|m| m["address"].as_str().unwrap().to_string())
        .expect("the moved half of the body");
    draw(&["recolour", path, &body_moved, "--to", "ink"]);
    assert_eq!(
        rows("l1")[2],
        "ibb.....iii.",
        "a recoloured pixel mark takes exactly the palette colour"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&draw(&["palette", path, "--json"]))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2,
        "and no shade is added to the palette"
    );
    draw(&["lasso", path, "l1", "--box", "8,1,3,4", "--flip", "h"]);
    assert_eq!(rows("l1")[2], "ibb.....iii.");
    draw(&["lasso", path, "l1", "--box", "0,0,3,4", "--rotate", "90"]);
    let turned = rows("l1");
    let count = |sign: char| turned.iter().flat_map(|r| r.chars()).filter(|c| *c == sign).count();
    assert_eq!(
        (count('i'), count('b')),
        (8 + 12 + 1, 4),
        "a quarter turn keeps every pixel: {turned:?}"
    );
    assert_eq!(&turned[1][..4], "iiii", "and the 3 by 4 piece now lies 4 by 3");
    let missing = run(&["strokes", path, "l1", "--entry", "teal"]);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("the palette has no"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pixels_are_shaded_dithered_outlined_and_replaced_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-shade-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let rows = || -> Vec<String> {
        draw(&["pixels", path, "l1", "--plain"])
            .lines()
            .skip_while(|line| !line.is_empty())
            .skip(1)
            .map(String::from)
            .collect()
    };
    draw(&["new", path, "--pixel", "12x8"]);
    draw(&["palette", path, "--ramp", "green", "--base", "#4c9a54", "--steps", "3"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&[
        "pixel", path, "l1", "--color", "green-2", "--rect", "4,3,3,2", "--filled",
    ]);
    draw(&["pixel", path, "l1", "--color", "ink", "--outline"]);
    assert_eq!(
        rows()[2..6],
        ["....iii.....", "...iGGGi....", "...iGGGi....", "....iii....."],
        "a one-pixel outline around the block"
    );
    draw(&["pixel", path, "l1", "--shade", "-1", "--rect", "4,4,3,1"]);
    draw(&["pixel", path, "l1", "--shade", "1", "--cells", "4,3"]);
    assert_eq!(
        rows()[3..5],
        ["...iaGGi....", "...igggi...."],
        "a highlight and a darker foot"
    );
    let first = serde_json::from_str::<serde_json::Value>(&draw(&["strokes", path, "l1", "--json"])).unwrap()["fills"]
        [0]["address"]
        .as_str()
        .unwrap()
        .to_string();
    draw(&["recolour", path, &first, "--step", "1"]);
    assert_eq!(
        rows()[3],
        "...iaaai....",
        "the block itself stepped lighter along its ramp"
    );
    draw(&["recolour", path, "--from", "ink", "--to", "green-1"]);
    assert_eq!(rows()[2], "....ggg.....", "every ink mark is the dark green now");
    draw(&[
        "pixel", path, "l1", "--color", "ink", "--second", "green-3", "--dither", "50", "--rect", "8,0,4,2", "--filled",
    ]);
    assert_eq!(
        rows()[..2],
        ["........iaia", "........aiai"],
        "a checkerboard of two colours"
    );
    let none = run(&["recolour", path, &first, "--step", "5"]);
    assert!(String::from_utf8_lossy(&none.stderr).contains("already at its end"));
    let both = run(&["pixel", path, "l1", "--shade", "1", "--color", "ink", "--cells", "1,1"]);
    assert!(!both.status.success());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_tile_wraps_at_its_edges_mirrors_and_renders_repeated_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-tile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("tile.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let rows = || -> Vec<String> {
        draw(&["pixels", path, "l1", "--plain"])
            .lines()
            .skip_while(|line| !line.is_empty())
            .skip(1)
            .map(String::from)
            .collect()
    };
    draw(&["new", path, "--pixel", "8x4", "--wrap"]);
    assert!(draw(&["canvas", path]).contains("wraps at its edges"));
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--rect", "6,3,4,2", "--filled"]);
    assert_eq!(
        rows(),
        ["ii....ii", "........", "........", "ii....ii"],
        "a block over the corner shows in all four corners"
    );
    draw(&[
        "pixel",
        path,
        "l1",
        "--color",
        "ink",
        "--cells",
        "1,1",
        "--mirror-x",
        "4",
        "--mirror-y",
        "2",
    ]);
    assert_eq!(rows()[1..3], [".i....i.", ".i....i."]);
    let shown = run(&["render", path, "--repeat", "3", "--scale", "4"]);
    assert!(
        String::from_utf8_lossy(&shown.stderr).contains("96x48"),
        "three tiles across and down at 4x: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    draw(&["canvas", path, "--wrap", "off"]);
    assert!(draw(&["canvas", path]).contains("stops at its edges"));
    let json: serde_json::Value = serde_json::from_str(&draw(&["canvas", path, "--json"])).unwrap();
    assert_eq!((json["width"].as_u64(), json.get("wrap")), (Some(8), None));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_pixel_layer_animates_frame_by_frame_moves_by_whole_pixels_and_refuses_in_betweens_and_bones() {
    let dir = std::env::temp_dir().join(format!("draw-binary-pixel-anim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("hop.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} went through");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    draw(&["new", path, "--pixel", "12x6", "--frames", "4", "--fps", "8"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--rect", "1,3,2,2", "--filled"]);
    draw(&["add", path, "cel", "--in", "l1", "frame=2"]);
    draw(&[
        "pixel", path, "l1", "--frame", "2", "--color", "ink", "--rect", "1,1,2,2", "--filled",
    ]);
    let row = |frame: &str, at: usize| -> String {
        draw(&["pixels", path, "l1", "--frame", frame, "--plain"])
            .lines()
            .skip_while(|line| !line.is_empty())
            .skip(1)
            .nth(at)
            .unwrap()
            .to_string()
    };
    assert_eq!(
        (row("0", 3), row("1", 3), row("2", 3), row("2", 1)),
        (
            ".ii.........".to_string(),
            ".ii.........".to_string(),
            "............".to_string(),
            ".ii.........".to_string()
        ),
        "each drawing holds until the next: frames 0 and 1 show the first, 2 and 3 the second"
    );
    draw(&["clip", path, "--add", "hop", "--from", "0", "--to", "3", "--loop"]);
    assert!(draw(&["frames", path]).contains("hop"));
    draw(&["key", path, "l1", "--frame", "0", "x=0"]);
    draw(&["key", path, "l1", "--frame", "3", "x=4"]);
    let hashes = draw(&["hash", path, "--all"]);
    assert_eq!(hashes.lines().count(), 4);
    let shown = |frame: &str| {
        let out = dir.join(format!("f{frame}.png"));
        draw(&["export", path, "--frame", frame, "-o", out.to_str().unwrap()]);
        std::fs::read(&out).unwrap()
    };
    assert_ne!(shown("0"), shown("1"), "the key moves the drawing between frames");
    let onion = run(&["render", path, "--frame", "2", "--overlay", "onion"]);
    assert!(onion.status.success(), "{}", String::from_utf8_lossy(&onion.stderr));
    let between = refused(&["inbetween", path, "--layers", "l1", "--from", "0", "--to", "2"]);
    assert!(
        between.contains("pixel layer") && between.contains("Draw the frames"),
        "{between}"
    );
    draw(&["add", path, "layer", "bones", "name=bones"]);
    let attached = refused(&["attach", path, "l1", "--bend", "--skeleton", "bones"]);
    assert!(
        attached.contains("pixel layer") && attached.contains("off the grid"),
        "{attached}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn klad_check_lists_what_stands_out_and_counts_the_details() {
    let dir = std::env::temp_dir().join(format!("draw-binary-check-quiet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("stud.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--pixel", "8x6"]);
    let grid = dir.join("stud.txt");
    std::fs::write(
        &grid,
        "a = #20222e\nb = #6a7088\nc = #c8cedc\ns = #ffcc33\n\naabbb...\naasbb...\nccccc...\n........\n........\n.......s\n",
    )
    .unwrap();
    draw(&["pixel", path, "l1", "--grid", grid.to_str().unwrap()]);
    let listed = draw(&["check", path]);
    assert!(
        listed.contains("frame 0: 1 to look at, 1 small one left out"),
        "{listed}"
    );
    assert!(
        listed.contains("7,5") && listed.contains("alone, outside the shape"),
        "{listed}"
    );
    assert!(
        listed.contains("1 left out: single pixels among other colours") && !listed.contains("2,1 "),
        "{listed}"
    );
    let found: serde_json::Value = serde_json::from_str(&draw(&["check", path, "--json"])).unwrap();
    let small: Vec<&serde_json::Value> = found["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["minor"] == true)
        .collect();
    assert_eq!(small.len(), 1, "{found}");
    assert_eq!(small[0]["at"], serde_json::json!([2, 1]));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_pixel_drawing_exports_sharp_at_whole_number_scales_as_png_gif_sheet_and_godot_scene() {
    let dir = std::env::temp_dir().join(format!("draw-binary-pixel-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("hop.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} went through");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    draw(&["new", path, "--pixel", "16x12", "--frames", "4", "--fps", "8"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["palette", path, "--add", "#ef233c", "--name", "red"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--rect", "2,6,4,4", "--filled"]);
    draw(&["pixel", path, "l1", "--color", "red", "--cells", "3,7 4,7"]);
    draw(&["add", path, "cel", "--in", "l1", "frame=2"]);
    draw(&[
        "pixel", path, "l1", "--frame", "2", "--color", "ink", "--rect", "2,2,4,4", "--filled",
    ]);
    draw(&["clip", path, "--add", "hop", "--from", "0", "--to", "3", "--loop"]);

    let between = refused(&["export", path, "--scale", "2.5"]);
    assert!(
        between.contains("whole-number scales") && between.contains("not 2.5"),
        "{between}"
    );
    assert!(refused(&["export", path, "--scale", "0.5"]).contains("whole-number scales"));
    assert!(refused(&["export", path, "--scale", "65"]).contains("past 64"));
    assert!(refused(&["export", path, "--gif", "--scale", "1.5"]).contains("whole-number scales"));
    let size = |file: &std::path::Path| {
        let png = std::fs::read(file).unwrap();
        (
            u32::from_be_bytes(png[16..20].try_into().unwrap()),
            u32::from_be_bytes(png[20..24].try_into().unwrap()),
        )
    };
    let big = dir.join("big.png");
    draw(&["export", path, "--scale", "32", "-o", big.to_str().unwrap()]);
    assert_eq!(size(&big), (512, 384), "past the old limit of 8");

    let named = draw(&["export", path, "--gif", "--scale", "8"]);
    let gif_path = std::path::PathBuf::from(named.trim());
    assert_eq!(
        gif_path.file_name().unwrap(),
        "hop-hop@8x.gif",
        "one GIF per clip, named for it"
    );
    let gif = std::fs::read(&gif_path).unwrap();
    assert_eq!(&gif[..6], b"GIF89a");
    assert_eq!(
        (
            u16::from_le_bytes([gif[6], gif[7]]),
            u16::from_le_bytes([gif[8], gif[9]])
        ),
        (128, 96)
    );
    assert_eq!(
        &gif[13..19],
        [0x22, 0x22, 0x3b, 0xef, 0x23, 0x3c],
        "the drawing's two colours, exactly"
    );
    let delays: Vec<u16> = gif
        .windows(8)
        .filter(|w| w[0] == 0x21 && w[1] == 0xf9 && w[2] == 4 && w[7] == 0)
        .map(|w| u16::from_le_bytes([w[4], w[5]]))
        .collect();
    assert_eq!(delays, [25, 25], "two drawings, each held two frames at 8 a second");
    assert!(
        gif.windows(11).any(|w| w == b"NETSCAPE2.0"),
        "the clip loops, so the GIF does"
    );
    let mixed = dir.join("mixed.gif");
    draw(&[
        "export",
        path,
        "--gif",
        "hop",
        "--scale",
        "8",
        "--dither",
        "-o",
        mixed.to_str().unwrap(),
    ]);
    assert_eq!(
        std::fs::read(&mixed).unwrap(),
        gif,
        "a drawing of a few colours is exact, so dithering changes nothing"
    );
    assert!(refused(&["export", path, "--dither"]).contains("--gif"));
    let range = dir.join("first.gif");
    draw(&["export", path, "--gif", "0-1", "-o", range.to_str().unwrap()]);
    assert_eq!(u16::from_le_bytes([std::fs::read(&range).unwrap()[6], 0]), 16);

    let sheet = dir.join("sheet.png");
    draw(&[
        "export",
        path,
        "--sheet",
        "all",
        "--scale",
        "3",
        "-o",
        sheet.to_str().unwrap(),
    ]);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("sheet.json")).unwrap()).unwrap();
    let first = &json["frames"][0];
    assert_eq!(
        (
            first["sourceSize"]["w"].as_u64(),
            first["frame"]["w"].as_u64(),
            first["frame"]["h"].as_u64()
        ),
        (Some(48), Some(12), Some(12)),
        "the sheet's cells are the drawing three times over: {first}"
    );

    let game = dir.join("game");
    draw(&["export", path, "--godot", game.to_str().unwrap(), "--scale", "2"]);
    let scene = std::fs::read_to_string(game.join("hop.tscn")).unwrap();
    assert!(
        scene.contains("type=\"AnimatedSprite2D\"") && scene.contains("texture_filter = 1\n"),
        "{scene}"
    );

    let folder = dir.join("into");
    std::fs::create_dir_all(&folder).unwrap();
    let into = folder.to_str().unwrap();
    draw(&["export", path, "--sheet", "all", "-o", into]);
    draw(&["export", path, "--gif", "hop", "--scale", "2", "-o", into]);
    draw(&["export", path, "--frame", "2", "--scale", "2", "-o", into]);
    draw(&["export", path, "--aseprite", "-o", into]);
    let mut inside: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    inside.sort();
    assert_eq!(
        inside,
        [
            "hop-f2@2x.png",
            "hop-hop@2x.gif",
            "hop-sheet.json",
            "hop-sheet.png",
            "hop.aseprite"
        ],
        "a folder that exists takes every kind of export under its usual name"
    );
    assert!(
        !dir.join("into.png").exists() && !dir.join("into.json").exists(),
        "nothing lands next to the folder"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn palette_variants_are_made_shown_and_exported_one_by_one() {
    let dir = std::env::temp_dir().join(format!("draw-binary-variants-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("team.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} went through");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    draw(&["new", path, "--pixel", "8x8", "--frames", "2", "--fps", "8"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["palette", path, "--add", "#c0392b", "--name", "team"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--rect", "1,1,6,6"]);
    draw(&["pixel", path, "l1", "--color", "team", "--rect", "2,2,4,4", "--filled"]);
    let row = |at: usize| -> String {
        draw(&["pixels", path, "l1", "--plain"])
            .lines()
            .skip_while(|line| !line.is_empty())
            .skip(1)
            .nth(at)
            .unwrap()
            .to_string()
    };
    let before = row(3);

    draw(&["palette", path, "--new-variant", "blue"]);
    draw(&["palette", path, "--recolour", "team", "--to", "#2a6fdb"]);
    let listed = draw(&["palette", path]);
    assert!(listed.starts_with("showing blue\n"), "{listed}");
    assert!(
        listed.contains("#2a6fdb") && listed.contains("variant blue: team #2a6fdb"),
        "{listed}"
    );
    assert_eq!(row(3), before, "the pixels are the same pixels, in another colour");
    draw(&["palette", path, "--variant", "base"]);
    let listed = draw(&["palette", path]);
    assert!(
        listed.starts_with("showing base\n") && listed.contains("#c0392b"),
        "{listed}"
    );
    assert!(refused(&["palette", path, "--variant", "pink"]).contains("base, blue"));
    assert!(refused(&["palette", path, "--to", "navy"]).contains("--rename-variant"));

    let pixel = |file: &std::path::Path| -> Vec<u8> { std::fs::read(file).unwrap() };
    let named: Vec<String> = draw(&["export", path, "--variants"])
        .lines()
        .map(String::from)
        .collect();
    let names: Vec<String> = named
        .iter()
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(names, ["team-f0.png", "team-blue-f0.png"]);
    assert_ne!(
        pixel(std::path::Path::new(&named[0])),
        pixel(std::path::Path::new(&named[1]))
    );
    let alone = dir.join("alone.png");
    draw(&["export", path, "--variant", "blue", "-o", alone.to_str().unwrap()]);
    assert_eq!(
        pixel(&alone),
        pixel(std::path::Path::new(&named[1])),
        "one variant by name, while the base is shown"
    );
    draw(&["palette", path, "--variant", "blue"]);
    let shown = dir.join("shown.png");
    draw(&["export", path, "-o", shown.to_str().unwrap()]);
    assert_eq!(
        pixel(&shown),
        pixel(&alone),
        "the same picture as showing the variant and exporting"
    );
    let base = dir.join("base.png");
    draw(&["export", path, "--variant", "base", "-o", base.to_str().unwrap()]);
    assert_eq!(
        pixel(&base),
        pixel(std::path::Path::new(&named[0])),
        "and the base while blue is shown"
    );

    let sheet = dir.join("out").join("sheet.png");
    std::fs::create_dir_all(sheet.parent().unwrap()).unwrap();
    let sheets = draw(&[
        "export",
        path,
        "--sheet",
        "all",
        "--variants",
        "-o",
        sheet.to_str().unwrap(),
    ]);
    assert!(
        sheets.contains("sheet.png") && sheets.contains("sheet-blue.png"),
        "{sheets}"
    );
    assert!(dir.join("out").join("sheet-blue.json").exists());
    let gifs = draw(&["export", path, "--gif", "--variants", "--scale", "2"]);
    assert!(
        gifs.contains("team@2x.gif") && gifs.contains("team-blue@2x.gif"),
        "{gifs}"
    );
    let table = |file: &str| std::fs::read(file.trim()).unwrap()[13..19].to_vec();
    let both: Vec<Vec<u8>> = gifs.lines().map(table).collect();
    assert_eq!(both[0], [0x22, 0x22, 0x3b, 0xc0, 0x39, 0x2b]);
    assert_eq!(both[1], [0x22, 0x22, 0x3b, 0x2a, 0x6f, 0xdb]);
    let game = dir.join("game");
    let godot = draw(&["export", path, "--godot", game.to_str().unwrap(), "--variants"]);
    assert!(
        godot.contains("team.tscn") && godot.contains("team-blue.tscn") && godot.contains("team-blue.png"),
        "{godot}"
    );

    draw(&["palette", path, "--rename-variant", "blue", "--to", "navy"]);
    assert!(draw(&["palette", path]).contains("variant navy: team #2a6fdb"));
    draw(&["palette", path, "--rm-variant", "navy"]);
    let listed = draw(&["palette", path]);
    assert!(!listed.contains("showing") && listed.contains("#c0392b"), "{listed}");
    assert!(refused(&["export", path, "--variant", "navy"]).contains("no palette variant"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_pixel_drawing_goes_out_to_an_aseprite_file_and_comes_back_the_same() {
    let dir = std::env::temp_dir().join(format!("draw-binary-aseprite-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("hero.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let both = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    let draw = |args: &[&str]| both(args).0;
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} went through");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    draw(&["new", path, "--pixel", "12x8", "--frames", "6", "--fps", "10"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["palette", path, "--add", "#ef233c", "--name", "red"]);
    draw(&["palette", path, "--add", "#f0c8a080", "--name", "glow"]);
    draw(&["set", path, "l1", "name=body"]);
    draw(&["pixel", path, "body", "--color", "ink", "--rect", "1,4,3,3", "--filled"]);
    draw(&["add", path, "cel", "--in", "l1", "frame=2"]);
    draw(&[
        "pixel", path, "body", "--frame", "2", "--color", "ink", "--rect", "1,1,3,3", "--filled",
    ]);
    draw(&[
        "pixel", path, "body", "--frame", "2", "--color", "red", "--cells", "2,2",
    ]);
    draw(&["add", path, "cel", "--in", "l1", "frame=5", "link=c2"]);
    let ops = dir.join("fx.jsonl");
    std::fs::write(
        &ops,
        r##"{"op":"add","layer":{"kind":"group","name":"fx","opacity":0.5,"children":[{"kind":"pixel","name":"spark","blend":"screen","visible":false,"cels":[{"frame":1,"hold":2}]}]}}"##,
    )
    .unwrap();
    draw(&["apply", path, "--file", ops.to_str().unwrap()]);
    draw(&[
        "pixel",
        path,
        "spark",
        "--frame",
        "1",
        "--color",
        "glow",
        "--cells",
        "8,1 9,1 9,2",
    ]);
    draw(&["add", path, "layer", "paint", "name=notes"]);
    draw(&["clip", path, "--add", "hop", "--from", "0", "--to", "4", "--loop"]);
    draw(&["clip", path, "--add", "land", "--from", "5", "--to", "5"]);

    let (named, said) = both(&["export", path, "--aseprite"]);
    let file = std::path::PathBuf::from(named.trim());
    assert_eq!(file.file_name().unwrap(), "hero.aseprite");
    assert!(said.contains("left out") && said.contains("notes"), "{said}");
    assert!(said.contains("3 layers") && said.contains("2 tags"), "{said}");
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 0xA5E0);
    assert_eq!(
        u16::from_le_bytes([bytes[6], bytes[7]]),
        5,
        "frames 0-1, 1 (the spark), 2, 3-4 and 5: a frame for each change, not for each klad frame"
    );

    let back = dir.join("back.klad");
    let (_, told) = both(&["import", file.to_str().unwrap(), "-o", back.to_str().unwrap()]);
    assert!(told.contains("6 frames at 10 fps"), "{told}");
    let again = back.to_str().unwrap();
    let grid = |drawing: &str, layer: &str, frame: u32| -> Vec<String> {
        draw(&["pixels", drawing, layer, "--frame", &frame.to_string(), "--plain"])
            .lines()
            .skip_while(|line| !line.is_empty())
            .skip(1)
            .map(String::from)
            .collect()
    };
    (0..6).for_each(|frame| {
        assert_eq!(
            grid(again, "body", frame),
            grid(path, "body", frame),
            "body, frame {frame}"
        );
        assert_eq!(
            grid(again, "spark", frame),
            grid(path, "spark", frame),
            "spark, frame {frame}"
        );
    });
    let listed = draw(&["ls", again]);
    assert!(
        listed.contains("group") && listed.contains("fx") && listed.contains("hidden"),
        "{listed}"
    );
    assert!(!listed.contains("notes"));
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(back.join("doc.json")).unwrap()).unwrap();
    let fx = doc["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "fx")
        .unwrap();
    assert!(
        (fx["opacity"].as_f64().unwrap() - 0.5).abs() < 0.003,
        "the group keeps its opacity, to the nearest of 255 steps: {fx}"
    );
    assert_eq!(
        fx["children"][0]["blend"], "screen",
        "and the layer its blend mode: {fx}"
    );
    let colours = draw(&["palette", again]);
    assert!(
        colours.contains("#f0c8a080") && colours.contains("glow") && colours.contains("ink"),
        "{colours}"
    );
    let timeline = draw(&["frames", again]);
    assert!(
        timeline.contains("clip hop  frames 0 to 4") && timeline.contains("loops"),
        "{timeline}"
    );
    assert!(timeline.contains("clip land  frames 5 to 5"), "{timeline}");
    assert!(timeline.contains("->"), "the linked drawing stays a link: {timeline}");
    assert_eq!(
        draw(&["hash", again, "--all"])
            .lines()
            .map(|l| l.split_whitespace().last().unwrap().to_string())
            .collect::<Vec<_>>(),
        draw(&["hash", path, "--all"])
            .lines()
            .map(|l| l.split_whitespace().last().unwrap().to_string())
            .collect::<Vec<_>>(),
        "every frame renders the same"
    );

    assert!(refused(&["import", file.to_str().unwrap(), "-o", again]).contains("is there already"));
    assert!(refused(&["import", path]).contains("Aseprite files"));
    let fake = dir.join("fake.ase");
    std::fs::write(&fake, b"not a sprite at all").unwrap();
    assert!(refused(&["import", fake.to_str().unwrap()]).contains("not an Aseprite file"));
    assert!(!dir.join("fake.klad").exists());
    let painting = dir.join("painting.klad");
    draw(&["new", painting.to_str().unwrap(), "--width", "64", "--height", "64"]);
    assert!(refused(&["export", painting.to_str().unwrap(), "--aseprite"]).contains("no pixel layers"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pictures_and_painted_layers_become_pixels_on_the_palette() {
    let dir = std::env::temp_dir().join(format!("draw-binary-pixelize-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("hop.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let both = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    let draw = |args: &[&str]| both(args).0;
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} went through");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let rows = |drawing: &str, layer: &str| -> Vec<String> {
        draw(&["pixels", drawing, layer, "--plain"])
            .lines()
            .skip_while(|line| !line.is_empty())
            .skip(1)
            .map(String::from)
            .collect()
    };
    draw(&["new", path, "--pixel", "16x12"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["palette", path, "--add", "#ef233c", "--name", "red"]);
    draw(&["pixel", path, "l1", "--color", "ink", "--rect", "2,6,4,4", "--filled"]);
    draw(&["pixel", path, "l1", "--color", "red", "--cells", "3,7 4,7"]);
    let png = dir.join("hop@8x.png");
    draw(&["export", path, "--scale", "8", "-o", png.to_str().unwrap()]);

    let back = dir.join("back.klad");
    let (named, told) = both(&[
        "import",
        png.to_str().unwrap(),
        "--size",
        "16",
        "-o",
        back.to_str().unwrap(),
    ]);
    assert_eq!(named.trim(), back.to_str().unwrap());
    assert!(told.contains("16x12, 2 palette colours"), "{told}");
    let again = back.to_str().unwrap();
    let swapped =
        |rows: Vec<String>| -> Vec<String> { rows.iter().map(|r| r.replace('a', "i").replace('b', "r")).collect() };
    assert_eq!(
        swapped(rows(again, "l1")),
        rows(path, "l1"),
        "a picture enlarged eight times comes back exact"
    );
    let own = draw(&["import", png.to_str().unwrap()]);
    assert!(own.trim().ends_with("hop@8x.klad"));
    assert!(
        draw(&["info", own.trim()]).contains("128x96"),
        "without --size a small picture keeps its size"
    );
    assert!(refused(&["import", png.to_str().unwrap(), "-o", again]).contains("is there already"));
    assert!(
        refused(&[
            "import",
            png.to_str().unwrap(),
            "--size",
            "900",
            "-o",
            dir.join("x.klad").to_str().unwrap()
        ])
        .contains("past 512")
    );
    assert!(refused(&["import", dir.join("notes.txt").to_str().unwrap()]).contains("Aseprite files"));

    let (_, told) = both(&[
        "pixelize",
        path,
        "--image",
        png.to_str().unwrap(),
        "--to",
        "l1",
        "--size",
        "8",
        "--at",
        "8,0",
    ]);
    assert!(
        told.contains("8x6 pixels in 1 colour on") && told.contains("nearest of the palette's 2 colours"),
        "at half the size the two red pixels are a quarter of their blocks, so the blocks are ink: {told}"
    );
    let placed = rows(path, "l1");
    assert_eq!(
        (placed[3].as_str(), placed[4].as_str(), placed[6].as_str()),
        (".........ii.....", ".........ii.....", "..iiii.........."),
        "the small copy sits beside the first, in the same palette colour"
    );
    assert_eq!(
        draw(&["palette", path]).lines().count(),
        2,
        "nothing was added to the palette"
    );
    let (_, told) = both(&[
        "pixelize",
        path,
        "--image",
        png.to_str().unwrap(),
        "--to",
        "l1",
        "--size",
        "8",
        "--colors",
        "2",
        "--at",
        "0,0",
    ]);
    assert!(
        told.contains("8x6 pixels in 2 colours"),
        "with colours made from the picture, the blocks that held a red pixel keep their own mix: {told}"
    );
    assert_eq!(
        draw(&["palette", path]).lines().count(),
        3,
        "the mixed colour joined the palette; ink was there already"
    );
    let quarter = refused(&[
        "pixelize",
        path,
        "--image",
        png.to_str().unwrap(),
        "--to",
        "l1",
        "--size",
        "4",
    ]);
    assert!(
        quarter.contains("no solid pixels"),
        "at a quarter of the size no block is half covered: {quarter}"
    );
    assert!(
        refused(&[
            "pixelize",
            path,
            "--image",
            png.to_str().unwrap(),
            "--to",
            "l1",
            "--at",
            "40,40"
        ])
        .contains("no solid pixels")
    );
    assert!(
        refused(&[
            "pixelize",
            path,
            "--image",
            png.to_str().unwrap(),
            "--to",
            "l1",
            "--colors",
            "1"
        ])
        .contains("2 to 256")
    );

    let painting = dir.join("sign.klad");
    let sign = painting.to_str().unwrap();
    draw(&[
        "new", sign, "--width", "320", "--height", "160", "--frames", "3", "--fps", "6",
    ]);
    draw(&["add", sign, "layer", "vector", "name=board"]);
    let board = draw(&["ls", sign])
        .lines()
        .find(|l| l.contains("board"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    draw(&[
        "add",
        sign,
        "rect",
        "--in",
        &board,
        "x=40",
        "y=40",
        "w=240",
        "h=80",
        "fill=#8b5a2b",
    ]);
    draw(&[
        "add",
        sign,
        "ellipse",
        "--in",
        &board,
        "cx=160",
        "cy=80",
        "rx=30",
        "ry=30",
        "fill=#f0c8a0",
    ]);
    draw(&["clip", sign, "--add", "sway", "--from", "0", "--to", "2", "--loop"]);
    assert!(refused(&["pixelize", sign]).contains("give --size"));
    let (named, told) = both(&["pixelize", sign, "--layer", "board", "--size", "32"]);
    let made = named.trim().to_string();
    assert!(made.ends_with("sign-board-pixel.klad"), "{made}");
    assert!(told.contains("32x16, 3 frames (1 drawing)"), "{told}");
    let pixels = rows(&made, "board");
    assert_eq!(pixels.len(), 16);
    assert_eq!(
        pixels[4],
        format!("....{}....", pixels[4][4..5].repeat(24)),
        "the board is one colour across, from 40 to 280 of 320"
    );
    assert_ne!(
        pixels[8].chars().nth(16),
        pixels[8].chars().nth(5),
        "with the disc in another colour"
    );
    assert!(
        pixels[0].chars().all(|c| c == '.'),
        "and nothing where the layer is clear"
    );
    let timeline = draw(&["frames", &made]);
    assert!(
        timeline.contains("3 frames at 6 fps") && timeline.contains("clip sway"),
        "{timeline}"
    );
    assert!(
        draw(&["ls", sign]).contains("board"),
        "the painting itself is unchanged"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_drawing_cut_into_tiles_is_read_drawn_and_exported_tile_by_tile() {
    let dir = std::env::temp_dir().join(format!("draw-binary-tiles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("game")).unwrap();
    let bundle = dir.join("ground.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let both = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    let draw = |args: &[&str]| both(args).0;
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} went through");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let odd = dir.join("odd.klad");
    assert!(
        refused(&["new", odd.to_str().unwrap(), "--pixel", "50x32", "--tile", "16"])
            .contains("does not cut into tiles")
    );
    assert!(!odd.exists(), "a drawing that cannot be cut is not left behind");
    draw(&["new", path, "--pixel", "48x32", "--tile", "16"]);
    assert!(draw(&["canvas", path]).contains("tiles of 16x16, 3 across and 2 down"));
    draw(&["palette", path, "--add", "#3a5a40", "--name", "grass"]);
    draw(&["palette", path, "--add", "#8b5a2b", "--name", "dirt"]);
    draw(&[
        "pixel",
        path,
        "l1",
        "--color",
        "grass",
        "--rect",
        "0,0,16,16",
        "--filled",
    ]);
    let grid = dir.join("tile.txt");
    std::fs::write(&grid, "d = dirt\n\ndddd\nd..d\ndddd\n").unwrap();
    draw(&["pixel", path, "l1", "--grid", grid.to_str().unwrap(), "--tile", "2,1"]);
    let whole = draw(&["pixels", path, "l1", "--plain"]);
    let rows: Vec<&str> = whole.lines().skip_while(|l| !l.is_empty()).skip(1).collect();
    assert_eq!(
        &rows[16][32..36],
        "dddd",
        "the grid went to the tile's own corner, canvas pixel 32,16"
    );
    assert_eq!(&rows[17][32..36], "d..d");
    let tile = draw(&["pixels", path, "l1", "--tile", "2,1", "--plain"]);
    let cut: Vec<&str> = tile.lines().skip_while(|l| !l.is_empty()).skip(1).collect();
    assert_eq!(cut.len(), 16, "one tile is 16 rows");
    assert_eq!(
        (cut[0], cut[1], cut[3]),
        ("dddd............", "d..d............", "................")
    );
    assert!(draw(&["pixels", path, "l1", "--tile", "2,1"]).contains("tile 2,1 (its top left is canvas pixel 32,16)"));
    assert!(refused(&["pixels", path, "l1", "--tile", "3,0"]).contains("3 by 2 tiles"));

    let art = dir.join("game").join("art");
    let (named, told) = both(&["export", path, "--tileset", art.to_str().unwrap()]);
    assert!(
        named.contains("ground-tiles.png") && named.contains("ground-tiles.tres"),
        "{named}"
    );
    assert!(
        told.contains("2 tiles of 16x16 (3 across, 2 down, 4 empty left out)"),
        "{told}"
    );
    assert!(told.contains("texture filter to Nearest"), "{told}");
    let tres = std::fs::read_to_string(art.join("ground-tiles.tres")).unwrap();
    assert!(
        tres.contains("tile_size = Vector2i(16, 16)") && tres.contains("0:0/0 = 0") && tres.contains("2:1/0 = 0"),
        "{tres}"
    );
    let png = std::fs::read(art.join("ground-tiles.png")).unwrap();
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 48);
    draw(&["export", path, "--tileset"]);

    draw(&["canvas", path, "--tile", "off"]);
    assert!(!draw(&["canvas", path]).contains("tiles of"));
    assert!(refused(&["export", path, "--tileset"]).contains("not cut into tiles"));
    assert!(refused(&["pixels", path, "l1", "--tile", "0,0"]).contains("not cut into tiles"));
    draw(&["canvas", path, "--tile", "24x16", "--wrap", "on"]);
    assert!(draw(&["canvas", path]).contains("wraps at its edges  tiles of 24x16, 2 across and 2 down"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn klad_check_lists_the_slips_of_pixel_art_with_their_places() {
    let dir = std::env::temp_dir().join(format!("draw-binary-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("slips.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} went through");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    draw(&["new", path, "--pixel", "24x16", "--frames", "3"]);
    draw(&["palette", path, "--add", "#1b1b2f", "--name", "dark"]);
    draw(&["palette", path, "--add", "#5a6e9a", "--name", "mid"]);
    draw(&["palette", path, "--add", "#cfd8ea", "--name", "light"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    draw(&["set", path, "l1", "name=body"]);
    assert!(draw(&["check", path]).contains("body (l1), frame 0: nothing to look at"));
    let grid = dir.join("slips.txt");
    std::fs::write(
        &grid,
        "d = dark\nm = mid\nl = light\ni = ink\n\n\
         dddddd..........i.......\n\
         mmmmmm..................\n\
         llllll..................\n\
         ........................\n\
         iiii....................\n\
         ....i...................\n\
         .....iiii...............\n\
         ........................\n\
         i.......................\n\
         .ii.....................\n\
         ..i.....................\n\
         ...i....................\n",
    )
    .unwrap();
    draw(&["pixel", path, "body", "--grid", grid.to_str().unwrap()]);
    draw(&["pixel", path, "body", "--color", "#ff00ff", "--cells", "20,12 21,12"]);
    draw(&["palette", path, "--rm", "#ff00ff"]);
    let listed = draw(&["check", path]);
    assert!(listed.contains("body (l1), frame 0:"), "{listed}");
    let line = |kind: &str| {
        listed
            .lines()
            .find(|l| l.trim_start().starts_with(kind))
            .unwrap_or_else(|| panic!("no {kind} in:\n{listed}"))
            .to_string()
    };
    assert!(
        line("stray pixel").contains("16,0") && line("stray pixel").contains("ink"),
        "{listed}"
    );
    assert!(line("doubled corner").contains("2,9"), "{listed}");
    assert!(
        line("jagged line").contains("4,5") && line("jagged line").contains("a run of 1 between runs of 4 and 4"),
        "{listed}"
    );
    assert!(
        line("banding").contains("0,1 to 5,1")
            && line("banding").contains("mid")
            && line("banding").contains("between dark and light"),
        "{listed}"
    );
    assert!(
        line("off the palette").contains("20,12")
            && line("off the palette").contains("#ff00ff")
            && line("off the palette").contains("2 pixels"),
        "{listed}"
    );
    assert!(listed.contains("not errors"), "{listed}");
    let found: serde_json::Value = serde_json::from_str(&draw(&["check", path, "body", "--json"])).unwrap();
    let kinds: std::collections::BTreeSet<&str> = found["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        ["banding", "corner", "jagged", "off-palette", "stray"]
    );
    assert_eq!(found["checked"][0]["name"], "body");
    draw(&["add", path, "cel", "--in", "l1", "frame=2"]);
    let every = draw(&["check", path, "--all"]);
    assert!(
        every.contains("frame 0:") && every.contains("body (l1), frame 2: nothing to look at"),
        "{every}"
    );
    assert_eq!(
        every.matches("body (l1), frame").count(),
        2,
        "each drawing once, not each frame: {every}"
    );
    assert!(refused(&["check", path, "--frame", "9"]).contains("past the last frame"));
    let painting = dir.join("painting.klad");
    draw(&["new", painting.to_str().unwrap(), "--width", "64", "--height", "64"]);
    assert!(refused(&["check", painting.to_str().unwrap()]).contains("no pixel layers"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn recolour_moves_strokes_together_and_keeps_their_shades() {
    let dir = std::env::temp_dir().join(format!("draw-binary-recolour-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    draw(&["stroke", path, "l1", "--path", "M10 10 L190 10", "--color", "#2f5a30"]);
    draw(&["stroke", path, "l1", "--path", "M10 50 L190 50", "--color", "#a8dc84"]);
    draw(&["recolour", path, "c2:s1", "c2:s2", "--to", "#e3962f"]);
    let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    let colours: Vec<String> = listed["strokes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["color"].as_str().unwrap().to_string())
        .collect();
    let lab = |hex: &str| klad_core::color::to_oklab(klad_core::color::parse_hex(hex).unwrap());
    assert!(lab(&colours[0])[0] < lab(&colours[1])[0], "{colours:?}");
    assert!(colours.iter().all(|c| lab(c)[2] > 0.05), "both warm now: {colours:?}");
    let log = draw(&["log", path]);
    assert_eq!(log.matches("cli").count(), 3, "one history step: {log}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_eraser_stroke_attaches_to_the_lines_it_crosses_and_says_so() {
    let dir = std::env::temp_dir().join(format!("draw-binary-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    draw(&["stroke", path, "l1", "--path", "M10 20 L190 20"]);
    draw(&["stroke", path, "l1", "--path", "M10 80 L190 80"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--brush",
        "eraser",
        "--size",
        "12",
        "--path",
        "M100 5 L100 35",
    ]);
    let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    let strokes = listed["strokes"].as_array().unwrap();
    assert_eq!(strokes[2]["on"], serde_json::json!(["c2:s1"]));
    assert_eq!(strokes[0]["attached"], serde_json::json!(["c2:s3"]));
    assert!(strokes[1].get("attached").is_none());
    assert!(draw(&["strokes", path, "l1"]).contains("on s1"));
    assert!(strokes.iter().all(|s| s.get("hidden").is_none()), "cut, not wiped");
    draw(&["stroke", path, "l1", "--path", "M150 50 L160 50"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--brush",
        "eraser",
        "--size",
        "40",
        "--path",
        "M140 50 L170 50",
    ]);
    let wiped: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    assert_eq!(wiped["strokes"][3]["hidden"], serde_json::json!(true));
    draw(&["detach", path, "c2:s1"]);
    let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    assert!(listed["strokes"][2].get("on").is_none(), "detached from its line");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn modifiers_move_go_and_copy_with_their_marks_and_attach_again_when_moved_alone() {
    let dir = std::env::temp_dir().join(format!("draw-binary-follow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let listed = |frame: &str| -> Vec<serde_json::Value> {
        let text = draw(&["strokes", path, "l1", "--frame", frame, "--json"]);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["strokes"].as_array().unwrap().clone()
    };
    draw(&["new", path, "--width", "400", "--height", "200"]);
    draw(&["stroke", path, "l1", "--path", "M10 40 L390 40"]);
    draw(&["stroke", path, "l1", "--path", "M10 140 L390 140"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--brush",
        "eraser",
        "--size",
        "12",
        "--path",
        "M100 25 L100 55",
    ]);
    draw(&["transform", path, "c2:s1", "--move", "0,20"]);
    let eraser = listed("0")[2].clone();
    assert_eq!(eraser["on"], serde_json::json!(["c2:s1"]));
    assert_eq!(
        eraser["bounds"][1],
        serde_json::json!(45.0),
        "the eraser came along with its line"
    );
    draw(&["transform", path, "c2:s3", "--move", "0,80"]);
    assert_eq!(
        listed("0")[2]["on"],
        serde_json::json!(["c2:s2"]),
        "moved on its own, it attaches to what it lands on"
    );
    draw(&["rm", path, "c2:s2"]);
    assert_eq!(listed("0").len(), 1, "the eraser went with the only line it was on");
    draw(&[
        "stroke",
        path,
        "l1",
        "--brush",
        "eraser",
        "--size",
        "12",
        "--path",
        "M200 45 L200 75",
    ]);
    let map = draw(&["copy", path, "c2:s1", "--frame", "4"]);
    assert_eq!(map.matches("->").count(), 2, "the line and its eraser: {map}");
    let copies = listed("4");
    assert_eq!(copies.len(), 2);
    assert_eq!(copies[1]["on"], serde_json::json!([copies[0]["address"]]));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_brush_with_a_feathered_or_fringed_edge_goes_in_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-feather-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let eraser = |feather: &str, fringe: &str| {
        format!(
            r#"{{"format":"klad-brush/1","name":"Eraser","tip":{{"round":{{"hardness":0.85}}}},"spacing":0.06,"blend":"erase","feather":{feather},"fringe":{fringe}}}"#
        )
    };
    let hash = |name: &str, brush: Option<String>| {
        let path = dir.join(format!("{name}.klad"));
        let path = path.to_str().unwrap();
        draw(&["new", path, "--width", "200", "--height", "100"]);
        draw(&[
            "fill",
            path,
            "l1",
            "--polygon",
            "0,0 200,0 200,100 0,100",
            "--color",
            "#204060",
        ]);
        let id = match brush {
            Some(def) => {
                let file = dir.join(format!("{name}.json"));
                std::fs::write(&file, def).unwrap();
                assert!(draw(&["brushes", path, "--set", "soft-eraser", file.to_str().unwrap()]).contains("added"));
                "soft-eraser"
            }
            None => "eraser",
        };
        draw(&[
            "stroke",
            path,
            "l1",
            "--brush",
            id,
            "--size",
            "40",
            "--path",
            "M20 50 L180 50",
        ]);
        let out = run(&["render", path, "-o", dir.join(format!("{name}.png")).to_str().unwrap()]);
        let said = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
        (
            draw(&["brushes", path]),
            said.split("hash ").nth(1).unwrap()[..16].to_string(),
        )
    };
    let (_, plain) = hash("plain", None);
    let (listed, feathered) = hash("feathered", Some(eraser("0.8", "0")));
    assert!(listed.contains("round 0.85 +feather"), "{listed}");
    assert_ne!(plain, feathered, "the feather softens the edge");
    let (listed, fringed) = hash("fringed", Some(eraser("0", "1")));
    assert!(listed.contains("round 0.85 +fringe"), "{listed}");
    assert_ne!(plain, fringed, "the fringe frays it");
    let file = dir.join("bad.json");
    std::fs::write(&file, eraser("2", "0")).unwrap();
    let path = dir.join("plain.klad");
    let out = run(&[
        "brushes",
        path.to_str().unwrap(),
        "--set",
        "bad",
        file.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("feather"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn light_maps_are_set_up_painted_lit_and_hashed_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-maps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("maps.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} should fail");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let id_of = |name: &str| -> String {
        draw(&["ls", path])
            .lines()
            .find(|line| line.split_whitespace().nth(2) == Some(name))
            .and_then(|line| line.split_whitespace().next())
            .unwrap()
            .to_string()
    };
    draw(&["new", path, "--width", "128", "--height", "128"]);
    draw(&["add", path, "layer", "vector", "name=body"]);
    let body = id_of("body");
    draw(&[
        "add",
        path,
        "ellipse",
        "--in",
        &body,
        "cx=64",
        "cy=64",
        "rx=40",
        "ry=40",
        "fill=#c08040",
    ]);
    assert!(draw(&["maps", path]).contains("maps are off"));
    let picture = draw(&["hash", path]);
    draw(&["maps", path, "--on"]);
    assert!(draw(&["maps", path]).contains("\"bevel\":8.0"));
    draw(&["maps", path, "bevel=12", "profile=chamfer"]);
    assert!(draw(&["maps", path]).contains("\"profile\":\"chamfer\""));
    assert!(refused(&["maps", path, "glow=1"]).contains("bevel, profile"));
    let png = dir.join("normal.png");
    draw(&["render", path, "--map", "normal", "-o", png.to_str().unwrap()]);
    assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
    let normal = draw(&["hash", path, "--map", "normal"]);
    assert!(normal.contains("normal"));
    draw(&["add", path, "layer", "vector", "name=bump", "map=height"]);
    let bump = id_of("bump");
    draw(&[
        "add",
        path,
        "ellipse",
        "--in",
        &bump,
        "cx=64",
        "cy=64",
        "rx=12",
        "ry=12",
        "fill=#ffffff",
    ]);
    assert_eq!(
        draw(&["hash", path]),
        picture,
        "a height layer is not drawn in the picture"
    );
    assert_ne!(
        draw(&["hash", path, "--map", "normal"]),
        normal,
        "but it shapes the normal map"
    );
    assert!(refused(&["render", path, "--lit"]).contains("no lights"));
    draw(&["lights", path, "--add", "point", "--at", "10,64", "--height", "40"]);
    draw(&["lights", path, "--add", "directional", "--at", "-1,-1"]);
    let listed = draw(&["lights", path]);
    assert!(listed.contains("1  point at 10,64 height 40 px") && listed.contains("2  directional from -1,-1"));
    draw(&["lights", path, "--set", "1", "--color", "#ffd9a0"]);
    assert!(draw(&["lights", path]).contains("#ffd9a0"));
    assert!(refused(&["lights", path, "--rm", "5"]).contains("no light 5"));
    draw(&["lights", path, "--rm", "2"]);
    assert!(!draw(&["lights", path]).contains("directional"));
    let lit = dir.join("lit.png");
    draw(&[
        "render",
        path,
        "--lit",
        "-o",
        lit.to_str().unwrap(),
        "--region",
        "0,32,64,64",
    ]);
    assert!(std::fs::read(&lit).unwrap().starts_with(b"\x89PNG"));
    assert!(refused(&["render", path, "--map", "glow"]).contains("emission, depth and occlusion"));
    assert!(refused(&["render", path, "--lit", "--overlay", "ids"]).contains("whole frame"));
    draw(&["set", path, &body, "surface.bevel=20"]);
    assert!(draw(&["show", path, &body]).contains("\"surface\""));
    assert!(refused(&["set", path, &body, "map=height", "surface.bevel=3"]).contains("no surface or depth"));
    draw(&["set", path, &body, "surface=null"]);
    draw(&["add", path, "layer", "map", "specular", "name=gloss"]);
    let gloss = id_of("gloss");
    let line = draw(&["ls", path])
        .lines()
        .find(|l| l.contains("gloss"))
        .unwrap()
        .to_string();
    assert!(line.contains(" map ") && line.contains("draws specular"), "{line}");
    assert!(refused(&["add", path, "layer", "map", "shiny"]).contains("height, normal, specular or emission"));
    draw(&["set", path, &gloss, "map=null"]);
    assert!(!draw(&["ls", path]).contains("draws specular"));
    draw(&["maps", path, "--off"]);
    assert!(draw(&["maps", path]).contains("maps are off"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn directions_shine_depth_surfaces_exports_and_checks_of_the_light_maps_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-maps-more-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("art.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} should fail");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let id_of = |name: &str| -> String {
        draw(&["ls", path])
            .lines()
            .find(|line| line.split_whitespace().nth(2) == Some(name))
            .and_then(|line| line.split_whitespace().next())
            .unwrap()
            .to_string()
    };
    draw(&["new", path, "--width", "128", "--height", "128", "--layers", "0"]);
    draw(&["add", path, "layer", "vector", "name=body"]);
    let body = id_of("body");
    draw(&[
        "add",
        path,
        "ellipse",
        "--in",
        &body,
        "cx=64",
        "cy=64",
        "rx=44",
        "ry=44",
        "fill=#c08040",
    ]);
    draw(&["maps", path, "--on"]);
    draw(&["add", path, "layer", "paint", "name=dents", "map=normal"]);
    let dents = id_of("dents");
    let flat = draw(&["hash", path, "--map", "normal"]);
    draw(&[
        "stroke",
        path,
        &dents,
        "--normal",
        "e",
        "--size",
        "14",
        "--path",
        "M40 64 L88 64",
    ]);
    assert_ne!(
        draw(&["hash", path, "--map", "normal"]),
        flat,
        "the stroke paints a direction"
    );
    draw(&[
        "stroke",
        path,
        &dents,
        "--facing",
        "0.3,-0.3",
        "--size",
        "6",
        "--path",
        "M50 40 L70 40",
    ]);
    assert!(
        refused(&["stroke", path, &dents, "--normal", "up-ish", "--path", "M1 1 L2 2"]).contains("not a direction")
    );
    draw(&[
        "set",
        path,
        &body,
        "shine=0.6",
        "specular=#ffd0a0",
        "emissive=true",
        "z=4",
    ]);
    let shown = draw(&["show", path, &body]);
    assert!(
        shown.contains("\"shine\": 0.6") && shown.contains("\"emissive\": true") && shown.contains("\"z\": 4.0"),
        "{shown}"
    );
    assert!(refused(&["set", path, &body, "shine=2"]).contains("shine 2"));
    assert!(refused(&["set", path, &dents, "emissive=true"]).contains("no shine, glow or z"));
    draw(&["surface", path, "body", "bevel=6", "profile=pillow"]);
    assert!(draw(&["surface", path, "body"]).contains("\"profile\":\"pillow\""));
    draw(&["surface", path, "body", "--off"]);
    assert!(draw(&["surface", path, "body"]).contains("part of the drawing's surface"));
    draw(&["surface", path, "bevel=10", "--directions", "9"]);
    assert!(draw(&["maps", path]).contains("\"directions\":9"));
    assert!(refused(&["surface", path, "--directions", "5"]).contains("9 or 17"));
    draw(&["surface", path, "--directions", "0"]);
    let depth = dir.join("depth.png");
    draw(&["render", path, "--map", "depth", "-o", depth.to_str().unwrap()]);
    let near = draw(&["hash", path, "--map", "depth"]);
    draw(&["set", path, &body, "z=40"]);
    assert_ne!(draw(&["hash", path, "--map", "depth"]), near, "z brings it nearer");
    draw(&[
        "render",
        path,
        "--map",
        "depth",
        "--invert",
        "-o",
        depth.to_str().unwrap(),
    ]);
    assert!(refused(&["render", path, "--invert"]).contains("--map"));
    let out = dir.join("art.png");
    draw(&[
        "export",
        path,
        "--maps",
        "normal,depth,occlusion",
        "-o",
        out.to_str().unwrap(),
    ]);
    let wide = std::fs::read(dir.join("art_d.png")).unwrap();
    assert_eq!((wide[24], wide[25]), (16, 4), "depth: 16-bit grey with alpha");
    assert!(dir.join("art_n.png").exists() && dir.join("art_o.png").exists());
    assert!(refused(&["export", path, "--maps", "glow"]).contains("no glow map"));
    assert!(refused(&["export", path, "--layer", &body, "--maps", "normal"]).contains("whole picture"));
    assert!(refused(&["export", path, "--occluders"]).contains("Godot export"));
    assert!(draw(&["check", path, "--maps"]).contains("look right"));
    draw(&["add", path, "layer", "vector", "name=lift", "map=height"]);
    let lift = id_of("lift");
    draw(&[
        "add",
        path,
        "ellipse",
        "--in",
        &lift,
        "cx=8",
        "cy=8",
        "rx=6",
        "ry=6",
        "fill=#ffffff",
    ]);
    let found = draw(&["check", path, "--maps", "--all", "--json"]);
    assert!(found.contains("\"kind\": \"stray\""), "{found}");
    let shapes = dir.join("shapes.png");
    draw(&["render", path, "--overlay", "occluders", "-o", shapes.to_str().unwrap()]);
    assert!(std::fs::read(&shapes).unwrap().starts_with(b"\x89PNG"));
    let pixel = dir.join("sprite.klad");
    let sprite = pixel.to_str().unwrap();
    draw(&["new", sprite, "--pixel", "16x16"]);
    draw(&[
        "pixel",
        sprite,
        "l1",
        "--color",
        "#808080",
        "--rect",
        "2,2,12,12",
        "--filled",
    ]);
    draw(&["add", sprite, "layer", "pixel", "name=dirs", "map=normal"]);
    let dirs = draw(&["ls", sprite])
        .lines()
        .find(|line| line.split_whitespace().nth(2) == Some("dirs"))
        .and_then(|line| line.split_whitespace().next())
        .unwrap()
        .to_string();
    draw(&["pixel", sprite, &dirs, "--normal", "nw", "--cells", "3,3"]);
    assert!(
        draw(&["palette", sprite]).contains("#40bfda"),
        "the north-west lean of the normal palette"
    );
    draw(&["pixel", sprite, &dirs, "--normal", "-0.9,-0.2", "--cells", "4,3"]);
    assert!(
        draw(&["palette", sprite]).contains("#2580da"),
        "x,y snaps to the west lean"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_in_steps_makes_one_edit_per_line_waits_between_them_and_goes_on_after_a_failure() {
    let dir = std::env::temp_dir().join(format!("draw-binary-steps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    let undo_steps = || -> usize {
        let log = draw(&["log", path]);
        let words: Vec<&str> = log.lines().next().unwrap().split_whitespace().collect();
        let at = words.iter().position(|w| *w == "undo").unwrap();
        words[at - 1].parse().unwrap()
    };
    let strokes = || -> u64 {
        let ls: serde_json::Value = serde_json::from_str(&draw(&["ls", path, "--json"])).unwrap();
        ls["layers"][0]["strokes"].as_u64().unwrap()
    };
    let line =
        |layer: &str, y: u32| format!(r#"{{"op":"stroke","layer":"{layer}","path":"M10 {y} L190 {y}","size":4}}"#);
    let script = |fifth: &str| {
        [
            line("l1", 10),
            "// the second part".to_string(),
            r#"{"pause":40}"#.to_string(),
            format!("[{},{}]", line("l1", 30), line("l1", 50)),
            line(fifth, 70),
            line("l1", 90),
        ]
        .join("\n")
    };
    let ops = dir.join("ops.jsonl");
    let file = ops.to_str().unwrap();
    std::fs::write(&ops, script("nope")).unwrap();
    let before = undo_steps();

    let whole = run(&["apply", path, "--file", file]);
    assert!(!whole.status.success(), "a pause is only a line of --steps");
    assert_eq!((strokes(), undo_steps()), (0, before), "one write: all or none");

    let started = std::time::Instant::now();
    let stopped = run(&["apply", path, "--file", file, "--steps", "--delay", "30"]);
    let said = String::from_utf8_lossy(&stopped.stderr).to_string();
    assert_eq!(stopped.status.code(), Some(1), "{said}");
    assert!(said.contains("line 5") && said.contains("--from 5"), "{said}");
    assert!(said.contains("the 2 before it"), "{said}");
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(100),
        "30 ms after each of two steps and a pause of 40"
    );
    assert_eq!(strokes(), 3, "the lines before the one that failed stay");
    assert_eq!(
        undo_steps(),
        before + 2,
        "a line is a step in the history, a list of ops one step"
    );

    std::fs::write(&ops, script("l1")).unwrap();
    let rest = draw(&["apply", path, "--file", file, "--from", "5"]);
    assert!(rest.starts_with("2 steps"), "{rest}");
    assert_eq!((strokes(), undo_steps()), (5, before + 4));
    draw(&["undo", path]);
    draw(&["undo", path]);
    draw(&["undo", path]);
    assert_eq!(strokes(), 1, "undoing the list takes both its strokes");

    let nothing = run(&["apply", path, "--file", file, "--from", "9"]);
    assert!(String::from_utf8_lossy(&nothing.stderr).contains("no steps from line 9"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn klad_pixel_writes_text_in_hard_edged_pixels() {
    let dir = std::env::temp_dir().join(format!("draw-binary-pixel-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sign.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--pixel", "32x16"]);
    let tree: serde_json::Value = serde_json::from_str(&draw(&["ls", path, "--json"])).unwrap();
    let layer = tree["layers"][0]["id"].as_str().unwrap().to_string();
    draw(&[
        "pixel",
        path,
        &layer,
        "--color",
        "#2b2d42",
        "--text",
        "HI",
        "--at",
        "2,1",
        "--text-size",
        "10",
    ]);
    let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, &layer, "--json"])).unwrap();
    let marks = listed["fills"].as_array().unwrap();
    assert_eq!(marks.len(), 1, "the text is one pixel mark");
    let cells = marks[0]["cells"].as_array().unwrap();
    assert!(cells.len() > 10, "the letters take pixels: {}", cells.len());
    let tiny = run(&[
        "pixel",
        path,
        &layer,
        "--color",
        "#2b2d42",
        "--text",
        ".",
        "--text-size",
        "0.5",
    ]);
    assert!(!tiny.status.success(), "a text too small for any pixel says so");
    assert!(String::from_utf8_lossy(&tiny.stderr).contains("larger --text-size"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn order_puts_marks_and_shapes_in_front_or_behind_and_strokes_paint_lines_and_figures() {
    let dir = std::env::temp_dir().join(format!("draw-binary-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    draw(&["stroke", path, "l1", "--line", "10,50,190,50", "--size", "4"]);
    draw(&["stroke", path, "l1", "--rect", "20,20,60,40", "--size", "4"]);
    draw(&["stroke", path, "l1", "--ellipse", "100,10,80,80", "--size", "4"]);
    let strokes = |draw: &dyn Fn(&[&str]) -> String| {
        let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
        listed["strokes"].as_array().unwrap().clone()
    };
    let listed = strokes(&draw);
    let ids: Vec<u64> = listed.iter().map(|s| s["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 3);
    let near = |b: &serde_json::Value, want: [f64; 4]| (0..4).all(|i| (b[i].as_f64().unwrap() - want[i]).abs() < 4.0);
    assert!(
        near(&listed[0]["bounds"], [10.0, 50.0, 190.0, 50.0]),
        "{}",
        listed[0]["bounds"]
    );
    assert!(
        near(&listed[1]["bounds"], [20.0, 20.0, 80.0, 60.0]),
        "{}",
        listed[1]["bounds"]
    );
    assert!(
        near(&listed[2]["bounds"], [100.0, 10.0, 180.0, 90.0]),
        "{}",
        listed[2]["bounds"]
    );
    let cel = listed[0]["address"]
        .as_str()
        .unwrap()
        .split(':')
        .next()
        .unwrap()
        .to_string();
    let first = format!("{cel}:s{}", ids[0]);
    let order = |draw: &dyn Fn(&[&str]) -> String| -> Vec<u64> {
        strokes(draw).iter().map(|s| s["id"].as_u64().unwrap()).collect()
    };
    draw(&["order", path, &first, "--front"]);
    assert_eq!(order(&draw), [ids[1], ids[2], ids[0]]);
    draw(&["order", path, &first, "--back"]);
    assert_eq!(order(&draw), [ids[0], ids[1], ids[2]]);
    draw(&["order", path, &first, "--up"]);
    assert_eq!(
        order(&draw),
        [ids[1], ids[0], ids[2]],
        "one place up: past the rectangle it overlaps"
    );
    let none = run(&["order", path, &first]);
    assert!(!none.status.success());
    assert!(String::from_utf8_lossy(&none.stderr).contains("--front, --back, --up or --down"));
    let ops = dir.join("shapes.jsonl");
    std::fs::write(
        &ops,
        r##"{"op":"add","layer":{"kind":"vector","name":"shapes"},"index":0}
"##,
    )
    .unwrap();
    draw(&["apply", path, "--file", ops.to_str().unwrap()]);
    let doc = |dir: &std::path::Path| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("a.klad").join("doc.json")).unwrap()).unwrap()
    };
    let layer = doc(&dir)["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "shapes")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    std::fs::write(
        &ops,
        format!(
            r##"{{"op":"add","node":{{"id":"red","type":"rect","x":0,"y":0,"w":50,"h":50,"fill":"#ff0000"}},"in":"{layer}","frame":0}}
{{"op":"add","node":{{"id":"blue","type":"rect","x":20,"y":20,"w":50,"h":50,"fill":"#0000ff"}},"in":"{layer}","frame":0}}
{{"op":"add","node":{{"id":"green","type":"rect","x":40,"y":40,"w":50,"h":50,"fill":"#00ff00"}},"in":"{layer}","frame":0}}
"##
        ),
    )
    .unwrap();
    draw(&["apply", path, "--file", ops.to_str().unwrap()]);
    let nodes = |dir: &std::path::Path| -> Vec<String> {
        let d = doc(dir);
        let l = d["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "shapes")
            .unwrap()
            .clone();
        l["cels"][0]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(nodes(&dir), ["red", "blue", "green"]);
    draw(&["order", path, "red", "--front"]);
    assert_eq!(nodes(&dir), ["blue", "green", "red"]);
    draw(&["order", path, "red", "blue", "--back"]);
    assert_eq!(
        nodes(&dir),
        ["blue", "red", "green"],
        "several keep their order among themselves"
    );
    draw(&["order", path, "green", "--down"]);
    assert_eq!(nodes(&dir), ["blue", "green", "red"]);
    let layer_too = run(&["order", path, &layer, "--front"]);
    assert!(String::from_utf8_lossy(&layer_too.stderr).contains("is a layer"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lasso_takes_an_ellipse_or_a_wand_and_pictures_go_through_the_clipboard() {
    let dir = std::env::temp_dir().join(format!("draw-binary-clip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let board = dir.join("board.png");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .env("KLAD_CLIPBOARD", &board)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--line",
        "20,20,60,20",
        "--size",
        "6",
        "--color",
        "#ff0000",
    ]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--line",
        "120,70,180,70",
        "--size",
        "6",
        "--color",
        "#0000ff",
    ]);
    let inside = draw(&["lasso", path, "l1", "--ellipse", "10,5,60,30"]);
    assert!(inside.contains("inside (1)"), "{inside}");
    let missed = run(&["lasso", path, "l1", "--at", "100,50", "--rm"]);
    assert!(!missed.status.success());
    assert!(String::from_utf8_lossy(&missed.stderr).contains("nothing is painted at 100,50"));
    draw(&["lasso", path, "l1", "--at", "150,70", "--rm"]);
    let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    assert_eq!(
        listed["strokes"].as_array().unwrap().len(),
        1,
        "the wand took the blue stroke"
    );

    draw(&["export", path, "--clipboard"]);
    let copied = std::fs::read(&board).unwrap();
    assert_eq!(&copied[1..4], b"PNG");
    let made = draw(&["import", "--clipboard"]);
    assert!(made.trim().ends_with("pasted.klad"), "{made}");
    let small: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("pasted.klad").join("doc.json")).unwrap()).unwrap();
    assert_eq!(
        small["layers"][0]["pixel"], true,
        "a small picture is a pixel drawing: {}",
        small["layers"][0]
    );

    let wide = dir.join("wide.klad");
    draw(&["new", wide.to_str().unwrap(), "--width", "700", "--height", "40"]);
    draw(&[
        "stroke",
        wide.to_str().unwrap(),
        "l1",
        "--line",
        "10,20,690,20",
        "--size",
        "8",
    ]);
    draw(&["export", wide.to_str().unwrap(), "--clipboard"]);
    draw(&["import", "--clipboard", "-o", "big.klad"]);
    let big: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("big.klad").join("doc.json")).unwrap()).unwrap();
    assert_eq!(
        (big["canvas"]["width"].as_u64(), big["canvas"]["height"].as_u64()),
        (Some(700), Some(40))
    );
    let kinds: Vec<&str> = big["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["kind"].as_str())
        .collect();
    assert_eq!(
        kinds.first(),
        Some(&"vector"),
        "the picture lies at the bottom: {kinds:?}"
    );
    assert_eq!(
        big["layers"][0]["cels"][0]["nodes"][0]["type"], "image",
        "a picture larger than 512 px comes in as a painting with the picture on a layer"
    );
    draw(&["add", path, "layer", "image", "src=clipboard"]);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(bundle.join("doc.json")).unwrap()).unwrap();
    assert!(doc["layers"].as_array().unwrap().iter().any(|l| l["name"] == "pasted"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shapes_take_a_look_combine_and_go_to_svg_and_back() {
    let dir = std::env::temp_dir().join(format!("draw-binary-svg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    draw(&["new", path, "--width", "300", "--height", "200"]);
    let ops = dir.join("shapes.jsonl");
    std::fs::write(
        &ops,
        "{\"op\":\"add\",\"layer\":{\"kind\":\"vector\",\"name\":\"shapes\"}}\n",
    )
    .unwrap();
    draw(&["apply", path, "--file", ops.to_str().unwrap()]);
    let doc = || -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(bundle.join("doc.json")).unwrap()).unwrap()
    };
    let layer = |d: &serde_json::Value, name: &str| -> serde_json::Value {
        d["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == name)
            .unwrap()
            .clone()
    };
    let id = layer(&doc(), "shapes")["id"].as_str().unwrap().to_string();
    std::fs::write(
        &ops,
        format!(
            r##"{{"op":"add","node":{{"id":"a","type":"rect","x":20,"y":20,"w":100,"h":80,"fill":"#ff0000","stroke":{{"paint":"#000000","width":4}}}},"in":"{id}","frame":0}}
{{"op":"add","node":{{"id":"b","type":"ellipse","cx":120,"cy":60,"rx":40,"ry":40,"fill":"#00ff00"}},"in":"{id}","frame":0}}
{{"op":"add","node":{{"id":"c","type":"rect","x":200,"y":120,"w":60,"h":50,"fill":"#0000ff"}},"in":"{id}","frame":0}}
{{"op":"add","node":{{"id":"t","type":"text","text":"Hi","font":"Inter","x":30,"y":130,"size":40,"fill":"#222222"}},"in":"{id}","frame":0}}
"##
        ),
    )
    .unwrap();
    draw(&["apply", path, "--file", ops.to_str().unwrap()]);
    draw(&["style", path, "a", "c"]);
    let shapes = |d: &serde_json::Value| layer(d, "shapes")["cels"][0]["nodes"].as_array().unwrap().clone();
    let now = shapes(&doc());
    assert_eq!(now[2]["fill"], "#ff0000");
    assert_eq!(now[2]["stroke"]["width"], 4.0, "style copies the outline too");

    let (svg, said) = draw(&["export", path, "--svg"]);
    assert!(said.contains("paint 1 went in as pictures"), "{said}");
    let file = dir.join(svg.trim());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("<rect id=\"a\"") && text.contains("<ellipse id=\"b\"") && text.contains("<text id=\"t\""));
    assert!(text.contains("data-name=\"shapes\"") && text.contains("data:image/png;base64,"));
    draw(&[
        "export",
        path,
        "--svg",
        "--vectors-only",
        "--outline-text",
        "-o",
        "flat.svg",
    ]);
    let flat = std::fs::read_to_string(dir.join("flat.svg")).unwrap();
    assert!(!flat.contains("<image") && !flat.contains("<text") && flat.contains("<path id=\"t\""));
    draw(&["export", path, "--svg", "--only", "b", "-o", "alone.svg"]);
    let (frames, _) = draw(&["export", path, "--svg", "--frames", "all"]);
    let folder = dir.join(frames.trim());
    assert!(
        folder.join("a-0000.svg").is_file(),
        "an SVG per frame in {}",
        folder.display()
    );
    let alone = std::fs::read_to_string(dir.join("alone.svg")).unwrap();
    assert!(alone.contains("<ellipse") && !alone.contains("<rect"));

    let (made, _) = draw(&["import", file.to_str().unwrap(), "-o", "back.klad"]);
    assert!(made.trim().ends_with("back.klad"));
    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("back.klad").join("doc.json")).unwrap()).unwrap();
    assert_eq!(back["canvas"]["width"], 300);
    let names: Vec<&str> = back["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["name"].as_str())
        .collect();
    assert_eq!(names, ["paint 1", "shapes"]);
    let back_shapes = layer(&back, "shapes");
    let types: Vec<&str> = back_shapes["cels"][0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["type"].as_str())
        .collect();
    assert_eq!(types, ["rect", "ellipse", "rect", "text"]);
    let (a, _) = draw(&["export", path, "-o", "a.png"]);
    let (b, _) = draw(&["export", dir.join("back.klad").to_str().unwrap(), "-o", "b.png"]);
    let compared = run(&["compare", a.trim(), b.trim(), "--json"]);
    let said = String::from_utf8_lossy(&compared.stdout).to_string();
    assert!(compared.status.success(), "{said}");
    draw(&["import", "flat.svg", "--into", path]);
    assert_eq!(
        doc()["layers"].as_array().unwrap().len(),
        3,
        "--into adds the SVG's layers"
    );

    draw(&["combine", path, "a", "b", "--unite"]);
    let combined = shapes(&doc());
    assert_eq!(combined.len(), 3);
    assert_eq!(combined[0]["type"], "path");
    assert_eq!(
        combined[0]["fill"], "#ff0000",
        "the path takes the look of the shape at the back"
    );
    let apart = run(&["combine", path, "c", "t", "--intersect"]);
    assert!(!apart.status.success());
    assert!(String::from_utf8_lossy(&apart.stderr).contains("nothing is left"));
    draw(&["path", path, "t", "--convert"]);
    let letters = shapes(&doc());
    assert!(letters.iter().all(|n| n["id"] != "t") && letters.iter().any(|n| n["name"] == "Hi"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merging_the_lemons_keeps_each_lemons_alpha_locked_strokes_on_that_lemon() {
    let dir = std::env::temp_dir().join(format!("draw-binary-lemons-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let generator = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/lemons/generate.py");
    let ops = ["python3", "python"]
        .iter()
        .find_map(|python| {
            Command::new(python)
                .arg(&generator)
                .output()
                .ok()
                .filter(|o| o.status.success())
        })
        .expect("python 3 runs samples/lemons/generate.py");
    std::fs::write(dir.join("lemons.jsonl"), &ops.stdout).unwrap();
    let bundle = dir.join("lemons.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    run(&["new", path, "--width", "1200", "--height", "900", "--layers", "0"]);
    run(&["apply", path, "--file", "lemons.jsonl"]);
    run(&["render", path, "-o", "before.png"]);
    let said = run(&["merge", path, "lemon-a", "lemon-b"]);
    assert!(
        said.contains("alpha-locked marks keep to the paint of their own layer"),
        "{said}"
    );
    assert!(
        said.contains("more (--json lists every one)") && said.len() < 2000,
        "merging thousands of strokes says so in a few lines: {} bytes",
        said.len()
    );
    run(&["render", path, "-o", "after.png"]);
    let out = Command::new(env!("CARGO_BIN_EXE_klad"))
        .args(["compare", "before.png", "after.png", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let difference = v["difference"].as_f64().unwrap();
    assert!(
        difference < 0.002,
        "lemon-b's strokes stay off lemon-a: {difference} (0.0096 before the fix)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn layers_merge_frame_by_frame_without_changing_the_picture_and_groups_come_apart() {
    let dir = std::env::temp_dir().join(format!("draw-binary-merge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    let same = |a: &str, b: &str| {
        let out = run(&["compare", a, b, "--json"]);
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["difference"].as_f64().unwrap()
    };
    draw(&["new", path, "--width", "200", "--height", "120", "--layers", "2"]);
    draw(&["add", path, "layer", "paint", "name=top"]);
    let ids = |path: &str| -> Vec<(String, String)> {
        let d: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(path).join("doc.json")).unwrap())
                .unwrap();
        d["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["id"].as_str().unwrap().to_string(),
                    l["name"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    let layers = ids(path);
    let (bottom, middle, top) = (layers[0].0.clone(), layers[1].0.clone(), layers[2].0.clone());
    draw(&["timeline", path, "--frames", "3"]);
    draw(&[
        "stroke",
        path,
        &bottom,
        "--line",
        "20,20,180,20",
        "--size",
        "8",
        "--color",
        "#ff0000",
    ]);
    draw(&[
        "stroke",
        path,
        &middle,
        "--line",
        "20,60,180,60",
        "--size",
        "8",
        "--color",
        "#00ff00",
    ]);
    draw(&[
        "stroke",
        path,
        &top,
        "--line",
        "100,80,100,110",
        "--size",
        "8",
        "--color",
        "#0000ff",
    ]);
    draw(&[
        "stroke",
        path,
        &top,
        "--line",
        "40,80,40,110",
        "--size",
        "8",
        "--color",
        "#0000ff",
        "--frame",
        "2",
    ]);
    draw(&["set", path, &top, "opacity=0.5"]);
    let render = |name: &str, frame: &str| {
        draw(&["export", path, "--frame", frame, "-o", name]);
        dir.join(name).to_string_lossy().to_string()
    };
    let (b0, b2) = (render("b0.png", "0"), render("b2.png", "2"));
    let (_, said) = draw(&["merge", path, "--down", &top]);
    assert!(
        said.contains("merged 2 layers into") && said.contains("opacity of top"),
        "{said}"
    );
    let (a0, a2) = (render("a0.png", "0"), render("a2.png", "2"));
    assert!(
        same(&a0, &b0) < 0.5 && same(&a2, &b2) < 0.5,
        "the picture is the same at every frame"
    );
    assert_eq!(ids(path).len(), 2);
    draw(&["merge", path, &middle, &bottom]);
    assert_eq!(ids(path).len(), 1);
    let (c0, c2) = (render("c0.png", "0"), render("c2.png", "2"));
    assert!(same(&c0, &b0) < 0.5 && same(&c2, &b2) < 0.5);

    draw(&["add", path, "layer", "vector", "name=shapes"]);
    let shapes = ids(path)[1].0.clone();
    let ops = dir.join("rect.jsonl");
    std::fs::write(
        &ops,
        format!(r##"{{"op":"add","node":{{"type":"rect","x":10,"y":70,"w":40,"h":40,"fill":"#ffaa00"}},"in":"{shapes}","frame":0}}"##),
    )
    .unwrap();
    draw(&["apply", path, "--file", ops.to_str().unwrap()]);
    let refused = run(&["merge", path, "--down", &shapes]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("layers merge with their own kind"));
    let (d0, d2) = (render("d0.png", "0"), render("d2.png", "2"));
    let (_, said) = draw(&["merge", path, "--visible"]);
    assert!(said.contains("one picture"), "{said}");
    let names: Vec<String> = ids(path).into_iter().map(|(_, n)| n).collect();
    assert_eq!(names, ["merged"]);
    let (e0, e2) = (render("e0.png", "0"), render("e2.png", "2"));
    assert!(
        same(&e0, &d0) < 0.5 && same(&e2, &d2) < 0.5,
        "merged as a picture, frame by frame"
    );

    draw(&["add", path, "layer", "group", "name=pair"]);
    draw(&["add", path, "layer", "paint", "name=one"]);
    draw(&["add", path, "layer", "paint", "name=two"]);
    let all = ids(path);
    let id_of = |name: &str| all.iter().find(|(_, n)| n == name).unwrap().0.clone();
    draw(&["mv", path, &id_of("one"), "--in", &id_of("pair")]);
    draw(&["mv", path, &id_of("two"), "--in", &id_of("pair")]);
    draw(&["set", path, &id_of("pair"), "opacity=0.5"]);
    draw(&["ungroup", path, "pair"]);
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(bundle.join("doc.json")).unwrap()).unwrap();
    let names: Vec<&str> = after["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["name"].as_str())
        .collect();
    assert_eq!(names, ["merged", "one", "two"]);
    assert_eq!(
        after["layers"][1]["opacity"], 0.5,
        "the group's opacity goes to its layers"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_canvas_grows_crops_scales_turns_and_flips_with_everything_on_it() {
    let dir = std::env::temp_dir().join(format!("draw-binary-canvas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let difference = |a: &str, b: &str| {
        let v: serde_json::Value = serde_json::from_slice(&run(&["compare", a, b, "--json"]).stdout).unwrap();
        v["difference"].as_f64().unwrap()
    };
    let size = |path: &str| {
        let d: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(path).join("doc.json")).unwrap()).unwrap();
        (
            d["canvas"]["width"].as_u64().unwrap(),
            d["canvas"]["height"].as_u64().unwrap(),
        )
    };
    let round_trip = |path: &str, steps: &[&[&str]]| {
        draw(&["export", path, "-o", "before.png"]);
        steps.iter().for_each(|step| {
            let mut args = vec!["canvas", path];
            args.extend_from_slice(step);
            draw(&args);
        });
        draw(&["export", path, "-o", "after.png"]);
        difference(
            &dir.join("after.png").to_string_lossy(),
            &dir.join("before.png").to_string_lossy(),
        )
    };

    draw(&["new", "paint.klad", "--width", "300", "--height", "200"]);
    draw(&[
        "stroke",
        "paint.klad",
        "l1",
        "--line",
        "20,30,200,150",
        "--size",
        "10",
        "--color",
        "#aa2233",
    ]);
    draw(&["add", "paint.klad", "layer", "vector", "name=shapes", "id=vec"]);
    std::fs::write(
        dir.join("rect.jsonl"),
        r##"{"op":"add","node":{"type":"rect","x":200,"y":20,"w":60,"h":40,"fill":"#2244aa","transform":{"rotate":20}},"in":"vec","frame":0}"##,
    )
    .unwrap();
    draw(&["apply", "paint.klad", "--file", "rect.jsonl"]);
    let back = round_trip(
        "paint.klad",
        &[
            &["--turn", "90"],
            &["--turn", "-90"],
            &["--flip", "h"],
            &["--flip", "h"],
            &["--turn", "180"],
            &["--turn", "180"],
            &["--size", "400,260", "--anchor", "bottom-right"],
            &["--crop", "100,60,300,200"],
            &["--crop", "-30,-20,360,240"],
            &["--crop", "30,20,300,200"],
            &["--resize", "2"],
            &["--resize", "0.5"],
        ],
    );
    assert!(
        back < 0.5,
        "turns, flips, sizes, crops and scales undo each other: {back}"
    );
    draw(&["canvas", "paint.klad", "--turn", "90"]);
    assert_eq!(size("paint.klad"), (200, 300));
    draw(&["canvas", "paint.klad", "--trim", "--paper", "#f4efe6"]);
    let (w, h) = size("paint.klad");
    assert!(w < 200 && h < 300, "trimmed to what is drawn: {w}x{h}");
    assert!(draw(&["canvas", "paint.klad"]).contains("background #f4efe6"));

    draw(&["new", "sprite.klad", "--pixel", "32x32", "--tile", "16"]);
    draw(&[
        "pixel",
        "sprite.klad",
        "l1",
        "--color",
        "#2b2d42",
        "--cells",
        "3-12,4 3,5-9",
    ]);
    let refused = run(&["canvas", "sprite.klad", "--size", "40,32"]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("whole numbers of tiles"));
    let refused = run(&["canvas", "sprite.klad", "--resize", "1.5"]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("whole numbers only"));
    let back = round_trip(
        "sprite.klad",
        &[
            &["--turn", "90"],
            &["--turn", "-90"],
            &["--size", "48,32", "--anchor", "left"],
            &["--crop", "0,0,32,32"],
            &["--flip", "v"],
            &["--flip", "v"],
        ],
    );
    assert!(back < 0.5, "a pixel sprite keeps its pixels: {back}");
    draw(&["canvas", "sprite.klad", "--resize", "2"]);
    assert_eq!(size("sprite.klad"), (64, 64));

    draw(&["new", "rig.klad", "--width", "400", "--height", "400", "--frames", "6"]);
    draw(&[
        "stroke",
        "rig.klad",
        "l1",
        "--path",
        "M100 100 L100 200 L100 300",
        "--size",
        "6",
    ]);
    draw(&["add", "rig.klad", "layer", "bones", "name=bones"]);
    draw(&[
        "bone",
        "rig.klad",
        "bones",
        "--chain",
        "thigh,shin",
        "--through",
        "100,100 100,200 100,300",
    ]);
    draw(&["attach", "rig.klad", "l1", "--bend", "--skeleton", "bones"]);
    draw(&["canvas", "rig.klad", "--size", "600,400", "--anchor", "right"]);
    draw(&[
        "pose", "rig.klad", "bones", "--from", "0", "--to", "3", "--bone", "shin=40",
    ]);
    let bones: serde_json::Value = serde_json::from_str(&draw(&["bones", "rig.klad", "--json"])).unwrap();
    let text = bones.to_string();
    assert!(text.contains("300"), "the bones moved with the canvas: {text}");
    draw(&["export", "rig.klad", "--frame", "3", "-o", "posed.png"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_grid_and_guides_are_kept_in_the_drawing_and_set_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-grid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let doc = || -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("a.klad").join("doc.json")).unwrap()).unwrap()
    };
    draw(&["new", "a.klad", "--width", "320", "--height", "240"]);
    assert!(draw(&["grid", "a.klad"]).contains("no grid"));
    draw(&[
        "grid", "a.klad", "--every", "16,8", "--sub", "4", "--show", "on", "--snap", "on",
    ]);
    let said = draw(&["grid", "a.klad"]);
    assert!(
        said.contains("every 16x8") && said.contains("shown, snapping on"),
        "{said}"
    );
    let d = doc();
    assert_eq!(d["format"], "klad/9");
    assert_eq!(d["guides"]["grid"]["every"], serde_json::json!([16.0, 8.0]));
    assert_eq!(d["guides"]["snap"], true);
    draw(&["guides", "a.klad", "--x", "120", "--x", "40", "--y", "64"]);
    let said = draw(&["guides", "a.klad"]);
    assert!(
        said.contains("upright at x: 40 120") && said.contains("level at y: 64"),
        "{said}"
    );
    draw(&["guides", "a.klad", "--rm-x", "40", "--locked", "on"]);
    assert_eq!(doc()["guides"]["x"], serde_json::json!([120.0]));
    assert_eq!(doc()["guides"]["locked"], true);
    std::fs::write(
        dir.join("g.jsonl"),
        "{\"op\":\"grid\",\"every\":32,\"iso\":true}\n{\"op\":\"guides\",\"clear\":true}\n",
    )
    .unwrap();
    draw(&["apply", "a.klad", "--file", "g.jsonl"]);
    assert_eq!(doc()["guides"]["grid"]["iso"], true);
    assert!(doc()["guides"].get("x").is_none());
    let bad = run(&["grid", "a.klad", "--every", "0"]);
    assert!(String::from_utf8_lossy(&bad.stderr).contains("1 to 4096"));
    draw(&["grid", "a.klad", "--off"]);
    assert!(draw(&["grid", "a.klad"]).contains("no grid"));
    draw(&["new", "tiles.klad", "--pixel", "64x64", "--tile", "16"]);
    draw(&["grid", "tiles.klad", "--show", "on"]);
    assert!(
        draw(&["grid", "tiles.klad"]).contains("every 16x16"),
        "a tiled drawing's grid starts at the tile size"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn drawing_guides_bend_strokes_and_your_brushes_follow_you_into_other_drawings() {
    let dir = std::env::temp_dir().join(format!("draw-binary-assist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib = dir.join("library");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .env("KLAD_LIBRARY", &lib)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let doc = |name: &str| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(name).join("doc.json")).unwrap()).unwrap()
    };
    draw(&["new", "a.klad", "--width", "640", "--height", "480"]);
    let refused = run(&["stroke", "a.klad", "l1", "--points", "10,10 50,50", "--follow"]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("no drawing guides"));
    draw(&[
        "guides",
        "a.klad",
        "--perspective",
        "-400,100,1000,100",
        "--ruler",
        "0,400,640,400",
    ]);
    let said = draw(&["guides", "a.klad"]);
    assert!(
        said.contains("drawing guide 0: 2-point perspective at -400,100 1000,100"),
        "{said}"
    );
    assert!(
        said.contains("drawing guide 1: a ruler") && said.contains("the brush follows them"),
        "{said}"
    );
    assert_eq!(doc("a.klad")["guides"]["assists"][0]["kind"], "perspective");
    draw(&[
        "stroke",
        "a.klad",
        "l1",
        "--points",
        "300,300 250,290 200,270 150,262",
        "--follow",
    ]);
    let strokes: serde_json::Value = serde_json::from_str(&draw(&["strokes", "a.klad", "l1", "--json"])).unwrap();
    let pts = strokes["strokes"][0]["points"].as_array().cloned().unwrap_or_default();
    assert!(pts.len() > 10, "the stroke is sampled along the guide: {strokes}");
    pts.iter().for_each(|p| {
        let (x, y) = (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32);
        let cross = (x - 300.0) * (100.0 - 300.0) - (y - 300.0) * (-400.0 - 300.0);
        assert!(
            cross.abs() / 700.0f32.hypot(200.0) < 1.0,
            "({x}, {y}) is on the line to the left vanishing point"
        );
    });
    draw(&["stroke", "a.klad", "l1", "--line", "100,405,500,396", "--follow"]);
    draw(&[
        "guides",
        "a.klad",
        "--rm-guide",
        "0",
        "--ellipse",
        "320,240,200,100",
        "--follow",
        "off",
    ]);
    let g = doc("a.klad")["guides"].clone();
    assert_eq!(g["assists"][0]["kind"], "ruler");
    assert_eq!(g["assists"][1]["kind"], "ellipse");
    assert!(g.get("follow").is_none());
    assert!(
        String::from_utf8_lossy(&run(&["guides", "a.klad", "--perspective", "1,2,3"]).stderr)
            .contains("one, two or three")
    );
    assert!(String::from_utf8_lossy(&run(&["guides", "a.klad", "--rm-guide", "7"]).stderr).contains("no guide 7"));

    assert!(draw(&["brushes", "--library"]).contains("no brushes of your own yet"));
    let mut def: serde_json::Value = serde_json::from_str(&draw(&["brushes", "--show", "ink"])).unwrap();
    def["name"] = "wet ink".into();
    def["spacing"] = 0.35.into();
    std::fs::write(dir.join("wet.json"), def.to_string()).unwrap();
    draw(&["brushes", "a.klad", "--set", "wet-ink", "wet.json"]);
    assert!(String::from_utf8_lossy(&run(&["brushes", "a.klad", "--save", "ink"]).stderr).contains("klad's own"));
    draw(&["brushes", "a.klad", "--save", "wet-ink"]);
    draw(&["brushes", "a.klad", "--save", "wet-ink", "--as", "wetter"]);
    draw(&[
        "brushes",
        "--rename",
        "wetter",
        "wetter ink",
        "--group",
        "wetter",
        "inks",
        "--move",
        "wetter",
        "-1",
    ]);
    let mine = draw(&["brushes", "--library"]);
    let lines: Vec<&str> = mine.lines().collect();
    assert!(
        lines[0].starts_with("wetter") && lines[0].contains("wetter ink") && lines[0].contains("[inks]"),
        "{mine}"
    );
    assert!(lines[1].starts_with("wet-ink"), "{mine}");
    draw(&["new", "b.klad", "--width", "200", "--height", "200"]);
    draw(&[
        "stroke",
        "b.klad",
        "l1",
        "--brush",
        "wet-ink",
        "--line",
        "10,10,150,150",
    ]);
    let kept: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("b.klad/brushes/wet-ink.json")).unwrap()).unwrap();
    assert_eq!(
        kept["spacing"], 0.35,
        "a stroke with one of your brushes gives the drawing its own copy"
    );
    draw(&["new", "c.klad", "--width", "64", "--height", "64"]);
    assert!(draw(&["brushes", "c.klad", "--take", "wetter"]).contains("its own copy"));
    assert!(dir.join("c.klad/brushes/wetter.json").exists());
    draw(&["brushes", "--export-library", "set.json"]);
    draw(&["brushes", "--forget", "wet-ink"]);
    draw(&["brushes", "--forget", "wetter"]);
    assert!(draw(&["brushes", "--library"]).contains("no brushes of your own yet"));
    assert!(draw(&["render", "b.klad", "--out", "b.png"]).is_empty() || dir.join("b.png").exists());
    assert!(draw(&["brushes", "--import-library", "set.json"]).contains("2 brushes added"));
    assert!(draw(&["brushes", "--library"]).contains("[inks]"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exports_write_jpeg_webp_tiff_pdf_with_shapes_kept_video_and_the_selection_alone() {
    let dir = std::env::temp_dir().join(format!("draw-binary-formats-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.klad");
    let path = path.to_str().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let refused = |args: &[&str]| {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} should be refused");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    draw(&["new", path, "--width", "120", "--height", "80", "--frames", "6"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--line",
        "10,60,110,60",
        "--size",
        "6",
        "--color",
        "#204080",
    ]);
    let ops = dir.join("shapes.jsonl");
    std::fs::write(
        &ops,
        "{\"op\":\"add\",\"layer\":{\"kind\":\"vector\",\"name\":\"shapes\"}}\n",
    )
    .unwrap();
    draw(&["apply", path, "--file", ops.to_str().unwrap()]);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("a.klad/doc.json")).unwrap()).unwrap();
    let layers = doc["layers"].as_array().unwrap();
    let shapes = layers.iter().find(|l| l["name"] == "shapes").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let cel = layers.iter().find(|l| l["id"] == "l1").unwrap()["cels"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    std::fs::write(
        &ops,
        format!("{{\"op\":\"add\",\"node\":{{\"id\":\"box\",\"type\":\"rect\",\"x\":20,\"y\":10,\"w\":50,\"h\":30,\"fill\":\"#c0392b\"}},\"in\":\"{shapes}\",\"frame\":0}}\n"),
    )
    .unwrap();
    draw(&["apply", path, "--file", ops.to_str().unwrap()]);
    let bytes = |name: &str| std::fs::read(dir.join(name)).unwrap();

    assert!(draw(&["export", path, "-o", "pic.jpg", "--quality", "80"]).ends_with("pic.jpg"));
    assert_eq!(&bytes("pic.jpg")[..3], &[0xFF, 0xD8, 0xFF]);
    assert!(draw(&["export", path, "--format", "webp", "--lossless"]).ends_with("a-f0.webp"));
    assert_eq!(&bytes("a-f0.webp")[8..16], b"WEBPVP8L");
    draw(&["export", path, "--format", "webp", "-o", "small.webp"]);
    assert_eq!(&bytes("small.webp")[8..15], b"WEBPVP8");
    draw(&["export", path, "--format", "tiff", "--depth", "16", "-o", "deep.tif"]);
    assert!(matches!(&bytes("deep.tif")[..4], b"II*\0" | b"MM\0*"));
    assert!(refused(&["export", path, "--format", "jpeg", "--depth", "16"]).contains("PNG and TIFF"));
    assert!(refused(&["export", path, "--format", "jpeg", "--gif"]).contains("for a picture"));
    assert!(refused(&["export", path, "--format", "bmp"]).contains("not a format"));
    assert!(refused(&["export", path, "--format", "jpeg", "--quality", "0"]).contains("1 to 100"));

    draw(&["export", path, "--format", "pdf", "-o", "one.pdf"]);
    let pdf = String::from_utf8_lossy(&bytes("one.pdf")).to_string();
    assert!(
        pdf.starts_with("%PDF-") && pdf.contains("/MediaBox [0 0 120 80]"),
        "{}",
        &pdf[..300.min(pdf.len())]
    );
    assert!(pdf.contains("/Subtype /Image"), "the paint layer is a picture");
    draw(&["set", path, "l1", "visible=false"]);
    draw(&["export", path, "--format", "pdf", "-o", "shapes.pdf"]);
    let shapes_only = String::from_utf8_lossy(&bytes("shapes.pdf")).to_string();
    assert!(
        !shapes_only.contains("/Subtype /Image"),
        "the shape is a path, not a picture"
    );
    draw(&["set", path, "l1", "visible=true"]);
    draw(&[
        "export",
        path,
        "--format",
        "pdf",
        "--pages",
        "all",
        "--paper",
        "a4,landscape",
        "-o",
        "all.pdf",
    ]);
    let all = String::from_utf8_lossy(&bytes("all.pdf")).to_string();
    assert!(all.contains("/Count 6") && all.contains("/MediaBox [0 0 842 595]"));
    draw(&["export", path, "--format", "pdf", "--dpi", "144", "-o", "half.pdf"]);
    assert!(String::from_utf8_lossy(&bytes("half.pdf")).contains("/MediaBox [0 0 60 40]"));
    assert!(refused(&["export", path, "--format", "pdf", "--pages", "clips"]).contains("no clips"));

    draw(&["key", path, "l1", "--frame", "0", "x=0"]);
    draw(&["key", path, "l1", "--frame", "3", "x=30"]);
    assert!(draw(&["export", path, "--format", "mp4"]).ends_with("a.mp4"));
    let mp4 = bytes("a.mp4");
    assert_eq!(&mp4[4..8], b"ftyp");
    assert!(mp4.windows(4).any(|w| w == b"av01") && mp4.windows(4).any(|w| w == b"mdat"));
    draw(&["export", path, "--format", "webp", "--animated", "-o", "moving.webp"]);
    assert!(bytes("moving.webp").windows(4).any(|w| w == b"ANIM"));

    assert!(draw(&["export", path, "--only", "box"]).ends_with("a-selection-f0.png"));
    let png = bytes("a-selection-f0.png");
    let size = |png: &[u8]| {
        (
            u32::from_be_bytes(png[16..20].try_into().unwrap()),
            u32::from_be_bytes(png[20..24].try_into().unwrap()),
        )
    };
    assert_eq!(size(&png), (50, 30), "cut to the shape");
    let strokes: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    let id = strokes["strokes"][0]["id"].as_u64().unwrap();
    let only = format!("{cel}:s{id}");
    draw(&[
        "export",
        path,
        "--only",
        &only,
        "--format",
        "webp",
        "--lossless",
        "-o",
        "stroke.webp",
    ]);
    let w = webp_size(&bytes("stroke.webp"));
    assert!(w.0 >= 100 && w.0 <= 112 && w.1 <= 12, "cut to the stroke: {w:?}");
    assert!(refused(&["export", path, "--only", "nothing"]).contains("no shape nothing"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn webp_size(data: &[u8]) -> (u32, u32) {
    let at = data.windows(4).position(|w| w == b"VP8L").unwrap() + 9;
    let bits = u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    ((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
}

#[test]
fn a_psd_goes_out_with_its_layers_and_comes_back_and_frames_come_in_from_a_sheet_or_numbered_pictures() {
    let dir = std::env::temp_dir().join(format!("draw-binary-psd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.klad");
    let path = path.to_str().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    let doc = |name: &str| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(name).join("doc.json")).unwrap()).unwrap()
    };
    draw(&["new", path, "--width", "96", "--height", "64"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--line",
        "10,50,86,20",
        "--size",
        "8",
        "--color",
        "#204080",
    ]);
    std::fs::write(
        dir.join("ops.jsonl"),
        "{\"op\":\"add\",\"layer\":{\"kind\":\"group\",\"name\":\"figure\",\"opacity\":0.8,\"children\":[{\"kind\":\"vector\",\"name\":\"body\",\"blend\":\"multiply\",\"cels\":[{\"frame\":0,\"nodes\":[{\"type\":\"rect\",\"x\":20,\"y\":10,\"w\":30,\"h\":30,\"fill\":\"#c0392b\"}]}]},{\"kind\":\"vector\",\"name\":\"hidden\",\"visible\":false,\"cels\":[{\"frame\":0,\"nodes\":[{\"type\":\"rect\",\"x\":0,\"y\":0,\"w\":8,\"h\":8,\"fill\":\"#00ff00\"}]}]}]}}\n",
    )
    .unwrap();
    draw(&["apply", path, "--file", "ops.jsonl"]);
    let (out, said) = draw(&["export", path, "-o", "a.psd"]);
    assert!(
        out.ends_with("a.psd") && said.contains("vector layers went in as pixels"),
        "{said}"
    );
    let psd = std::fs::read(dir.join("a.psd")).unwrap();
    assert_eq!(&psd[..4], b"8BPS");
    let (back, said) = draw(&["import", "a.psd", "-o", "back.klad"]);
    assert!(back.ends_with("back.klad"), "{said}");
    let layers = doc("back.klad")["layers"].clone();
    let names: Vec<&str> = layers
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap_or(""))
        .collect();
    assert!(names.contains(&"figure"), "{names:?}");
    let figure = layers
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "figure")
        .unwrap();
    assert_eq!(figure["kind"], "group");
    assert!((figure["opacity"].as_f64().unwrap() - 0.8).abs() < 0.01);
    let kids = figure["children"].as_array().unwrap();
    let body = kids.iter().find(|l| l["name"] == "body").unwrap();
    assert_eq!(body["blend"], "multiply");
    assert_eq!(kids.iter().find(|l| l["name"] == "hidden").unwrap()["visible"], false);
    let (a, _) = draw(&["export", path, "-o", "a.png"]);
    let (b, _) = draw(&["export", dir.join("back.klad").to_str().unwrap(), "-o", "b.png"]);
    let compared = run(&["compare", &a, &b, "--json"]);
    assert!(
        compared.status.success(),
        "the PSD brings back the same picture: {}",
        String::from_utf8_lossy(&compared.stdout)
    );
    let photoshop =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../render/testdata/psd/green-clipping-10x10.psd");
    std::fs::copy(&photoshop, dir.join("clip.psd")).unwrap();
    draw(&["import", "clip.psd"]);
    let clipped = doc("clip.klad")["layers"].clone();
    assert!(
        clipped
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["name"] == "First clipped layer" && l["clip"] == true)
    );
    assert!(String::from_utf8_lossy(&run(&["import", "a.psd", "--into", path]).stderr).contains("drawing of its own"));

    draw(&["new", "sprite.klad", "--pixel", "8x6", "--frames", "4"]);
    (0..4).for_each(|i| {
        let frame = i.to_string();
        let rect = format!("{},1,2,2", i * 2);
        draw(&[
            "pixel",
            "sprite.klad",
            "l1",
            "--frame",
            &frame,
            "--color",
            "#ff0000",
            "--rect",
            &rect,
            "--filled",
        ]);
    });
    draw(&["export", "sprite.klad", "--frames", "all", "-o", "frames"]);
    let numbered = std::fs::read_dir(dir.join("frames")).unwrap().count();
    assert!(numbered >= 1);
    let (made, said) = draw(&["import", "frames"]);
    assert!(
        made.ends_with("frames.klad"),
        "a folder makes a drawing named after it, beside it: {made} {said}"
    );
    let from_folder = doc("frames.klad");
    assert_eq!(from_folder["timeline"]["frames"], 4);
    assert!(
        from_folder["timeline"]["clips"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "sprite" && c["to"] == 3),
        "a clip of the frames"
    );

    draw(&["new", "sheet.klad", "--pixel", "26x13"]);
    [(0usize, 0usize), (1, 0), (2, 0), (0, 1)].iter().for_each(|(c, r)| {
        let rect = format!("{},{},6,4", 1 + c * 8, 1 + r * 6);
        let colour = format!("#{:02x}c8{:02x}", 40 * c, 90 * r);
        draw(&[
            "pixel",
            "sheet.klad",
            "l1",
            "--color",
            &colour,
            "--rect",
            &rect,
            "--filled",
        ]);
    });
    draw(&["export", "sheet.klad", "-o", "sheet.png"]);
    let (_, said) = draw(&[
        "import",
        "sheet.png",
        "--cell",
        "6x4",
        "--padding",
        "2",
        "--margin",
        "1",
        "--to",
        "sprite.klad",
    ]);
    assert!(said.contains("4 frames on a new layer sheet"), "{said}");
    let sprite = doc("sprite.klad");
    let sheet_layer = sprite["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "sheet")
        .unwrap()
        .clone();
    assert_eq!(sheet_layer["pixel"], true);
    assert_eq!(sheet_layer["cels"].as_array().unwrap().len(), 4);
    assert!(
        sprite["timeline"]["clips"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "sheet" && c["to"] == 3)
    );
    assert!(
        String::from_utf8_lossy(&run(&["import", "sheet.png", "--cell", "6x4", "--grid", "3x2"]).stderr)
            .contains("not both")
    );
    assert!(
        String::from_utf8_lossy(&run(&["import", "sheet.png", "--padding", "2"]).stderr).contains("go with --cell")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drawing_is_saved_as_a_copy_with_its_own_ids_and_renamed() {
    let dir = std::env::temp_dir().join(format!("draw-binary-save-as-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    draw(&["new", "a.klad", "--width", "64", "--height", "64"]);
    draw(&["stroke", "a.klad", "l1", "--line", "4,4,60,60"]);
    let made = draw(&["save-as", "a.klad", "b"]);
    assert!(made.ends_with("b.klad"), "{made}");
    assert!(dir.join("b.klad/doc.json").is_file());
    let tag = |name: &str| -> String {
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(name).join(".cache/copy.json")).unwrap()).unwrap();
        state["tag"].as_str().unwrap_or("").to_string()
    };
    assert_ne!(tag("a.klad"), tag("b.klad"), "the copy makes ids of its own");
    draw(&["stroke", "b.klad", "l1", "--line", "60,4,4,60"]);
    let strokes = |name: &str| -> usize {
        let v: serde_json::Value = serde_json::from_str(&draw(&["strokes", name, "l1", "--json"])).unwrap();
        v["strokes"].as_array().map_or(0, |a| a.len())
    };
    assert_eq!(
        (strokes("a.klad"), strokes("b.klad")),
        (1, 2),
        "the two are separate drawings"
    );
    assert!(String::from_utf8_lossy(&run(&["save-as", "a.klad", "b"]).stderr).contains("there already"));
    assert!(draw(&["rename", "b.klad", "hero"]).ends_with("hero.klad"));
    assert!(!dir.join("b.klad").exists() && dir.join("hero.klad/doc.json").is_file());
    assert!(String::from_utf8_lossy(&run(&["rename", "nothing.klad", "x"]).stderr).contains("not a klad drawing"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn canvases_too_big_for_the_machine_are_refused_and_running_out_of_memory_says_so() {
    let dir = std::env::temp_dir().join(format!("draw-binary-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str], gb: &str| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .env("KLAD_MEMORY_GB", gb)
            .output()
            .unwrap()
    };
    let big = run(&["new", "big.klad", "--width", "16384", "--height", "16384"], "4");
    let said = String::from_utf8_lossy(&big.stderr).to_string();
    assert!(
        !big.status.success() && said.contains("needs about 8.7 GB") && said.contains("4.0 GB"),
        "{said}"
    );
    assert!(!dir.join("big.klad").exists(), "nothing is made");
    let tight = run(&["new", "tight.klad", "--width", "6000", "--height", "6000"], "2");
    assert!(tight.status.success());
    assert!(
        String::from_utf8_lossy(&tight.stderr).contains("may slow down"),
        "a tight canvas is made with a warning"
    );
    let grown = run(&["canvas", "tight.klad", "--size", "16384,16384"], "4");
    assert!(!grown.status.success() && String::from_utf8_lossy(&grown.stderr).contains("needs about"));
    assert!(
        run(&["new", "fine.klad", "--width", "1024", "--height", "1024"], "2")
            .stderr
            .is_empty()
    );
    if cfg!(target_os = "linux") {
        let exe = env!("CARGO_BIN_EXE_klad");
        let shell = |script: String| {
            Command::new("sh")
                .arg("-c")
                .arg(script)
                .current_dir(&dir)
                .env("KLAD_MEMORY_GB", "64")
                .env("XDG_STATE_HOME", dir.join("state"))
                .output()
                .unwrap()
        };
        let out = shell(format!(
            "'{exe}' stress st.klad --size 4096 --layers 4 >/dev/null && ulimit -v 400000 && '{exe}' render st.klad -o st.png"
        ));
        let said = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(!out.status.success(), "{said}");
        assert!(
            said.contains("klad ran out of memory") && said.contains("saved in the drawing"),
            "running out of memory ends with words: {said}"
        );
        let note = std::fs::read_to_string(dir.join("state/klad/out-of-memory.txt")).unwrap();
        assert!(
            note.contains("klad ran out of memory"),
            "and leaves a note the editor shows the next time"
        );
        let out = shell(format!(
            "ulimit -v 600000; '{exe}' new huge.klad --width 2048 --height 2048 && '{exe}' render huge.klad --scale 8 -o huge.png"
        ));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("not enough memory"),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_posed_fill_keeps_a_smooth_edge_where_its_bone_does_not_move() {
    let dir = std::env::temp_dir().join(format!("draw-binary-posed-edge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    draw(&["new", "a.klad", "--width", "400", "--height", "400", "--layers", "0"]);
    let ellipse = |cx: f32, cy: f32, rx: f32, ry: f32| -> String {
        let pts: Vec<String> = (0..40)
            .map(|k| {
                let t = std::f32::consts::TAU * k as f32 / 40.0;
                format!("[{:.1},{:.1}]", cx + rx * t.cos(), cy + ry * t.sin())
            })
            .collect();
        format!("[{}]", pts.join(","))
    };
    let ops = [
        r#"{"op":"timeline","frames":4}"#.to_string(),
        r#"{"op":"add","layer":{"id":"colour","kind":"paint","name":"colour"}}"#.to_string(),
        r#"{"op":"add","layer":{"id":"bones","kind":"bones","name":"bones"}}"#.to_string(),
        format!(r##"{{"op":"fill","layer":"colour","polygon":{},"color":"#3f6f8f","part":"body"}}"##, ellipse(200.0, 235.0, 45.0, 65.0)),
        r##"{"op":"stroke","layer":"colour","brush":"round-bristle","size":22,"color":"#2c4f66","path":"M226 195 L 280 180","part":"arm"}"##.to_string(),
        r#"{"op":"bone","layer":"bones","add":"body","head":[200,295],"tail":[200,180]}"#.to_string(),
        r#"{"op":"bone","layer":"bones","add":"arm","parent":"body","head":[226,196],"tail":[280,180]}"#.to_string(),
        r#"{"op":"attach","layer":"colour","skeleton":"bones"}"#.to_string(),
        r#"{"op":"weights","layer":"bones","auto":true}"#.to_string(),
        r#"{"op":"weights","layer":"bones","bone":"body","part":"body"}"#.to_string(),
        r#"{"op":"pose","layer":"bones","from":0,"to":2,"bones":{"arm":-50}}"#.to_string(),
    ];
    [&ops[..5], &ops[5..8], &ops[8..10], &ops[10..]]
        .iter()
        .for_each(|stage| {
            std::fs::write(dir.join("rig.jsonl"), stage.join("\n")).unwrap();
            draw(&["apply", "a.klad", "--file", "rig.jsonl"]);
        });
    let hashed = |frame: &str, region: &str| -> String {
        let out = run(&["render", "a.klad", "--frame", frame, "--region", region, "-o", "x.png"]);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let words: Vec<&str> = said.split_whitespace().collect();
        let at = words.iter().position(|w| *w == "hash").expect("render says its hash");
        words[at + 1].to_string()
    };
    let hash = |frame: &str| hashed(frame, "150,200,50,100");
    assert_eq!(
        hash("0"),
        hash("2"),
        "the body's left side, which its bone does not move, renders as it does at rest"
    );
    let arm = |frame: &str| hashed(frame, "230,140,80,70");
    assert_ne!(arm("0"), arm("2"), "while the arm moves");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pose_in_a_batch_with_a_fill_is_refused_with_what_to_do() {
    let dir = std::env::temp_dir().join(format!("draw-binary-pose-batch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    assert!(
        run(&["new", "a.klad", "--width", "200", "--height", "200", "--layers", "0"])
            .status
            .success()
    );
    let stage = |text: &str| {
        std::fs::write(dir.join("ops.jsonl"), text).unwrap();
        run(&["apply", "a.klad", "--file", "ops.jsonl"])
    };
    assert!(stage("{\"op\":\"add\",\"layer\":{\"id\":\"colour\",\"kind\":\"paint\",\"name\":\"colour\"}}\n{\"op\":\"add\",\"layer\":{\"id\":\"bones\",\"kind\":\"bones\",\"name\":\"bones\"}}\n{\"op\":\"bone\",\"layer\":\"bones\",\"add\":\"arm\",\"head\":[50,100],\"tail\":[150,100]}\n{\"op\":\"attach\",\"layer\":\"colour\",\"skeleton\":\"bones\"}").status.success());
    let mixed = stage(
        "{\"op\":\"fill\",\"layer\":\"colour\",\"polygon\":[[40,90],[160,90],[160,110],[40,110]],\"color\":\"#336699\"}\n{\"op\":\"pose\",\"layer\":\"bones\",\"from\":0,\"to\":0,\"bones\":{\"arm\":20}}",
    );
    let said = String::from_utf8_lossy(&mixed.stderr).to_string();
    assert!(!mixed.status.success());
    assert!(
        said.contains("op 2: a pose needs a batch of its own") && said.contains("apply the drawing first"),
        "{said}"
    );
    assert!(
        !said.contains("computed by klad pose"),
        "the old message sent people to the command they ran"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn weights_are_added_taken_away_with_strength_and_read_at_a_point() {
    let dir = std::env::temp_dir().join(format!("draw-binary-weights-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    draw(&["new", "a.klad", "--width", "200", "--height", "300"]);
    std::fs::write(
        dir.join("rig.jsonl"),
        "{\"op\":\"add\",\"layer\":{\"id\":\"bones\",\"kind\":\"bones\",\"name\":\"bones\"}}\n{\"op\":\"bone\",\"layer\":\"bones\",\"add\":\"thigh\",\"head\":[100,40],\"tail\":[100,150]}\n{\"op\":\"bone\",\"layer\":\"bones\",\"add\":\"shin\",\"parent\":\"thigh\",\"head\":[100,150],\"tail\":[100,270]}\n{\"op\":\"attach\",\"layer\":\"l1\",\"skeleton\":\"bones\"}",
    )
    .unwrap();
    draw(&["apply", "a.klad", "--file", "rig.jsonl"]);
    let read = |at: &str| -> f32 {
        let said = draw(&["weights", "a.klad", "bones", "--bone", "shin", "--at", at]);
        let painted = said.split("painted ").nth(1).unwrap().split(',').next().unwrap();
        painted.trim().parse().unwrap()
    };
    assert!(read("100,200") < 0.01);
    draw(&[
        "weights",
        "a.klad",
        "bones",
        "--bone",
        "shin",
        "--set",
        "100,200,30,1",
        "--mode",
        "add",
        "--strength",
        "0.4",
    ]);
    let once = read("100,200");
    assert!((0.3..0.5).contains(&once), "a dab at strength 0.4 adds 0.4: {once}");
    draw(&[
        "weights",
        "a.klad",
        "bones",
        "--bone",
        "shin",
        "--set",
        "100,200,30,0.25",
        "--mode",
        "remove",
    ]);
    assert!((read("100,200") - (once - 0.25)).abs() < 0.03);
    let said = draw(&["weights", "a.klad", "bones", "--bone", "shin", "--at", "100,200"]);
    assert!(
        said.starts_with("shin at 100,200: painted") && said.contains("used"),
        "{said}"
    );
    let bad = run(&[
        "weights",
        "a.klad",
        "bones",
        "--bone",
        "shin",
        "--set",
        "100,200,30,1",
        "--strength",
        "2",
    ]);
    assert!(String::from_utf8_lossy(&bad.stderr).contains("0 to 1"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_preview_for_finder_is_written_sized_kept_out_of_git_and_skipped_when_nothing_changed() {
    let dir = std::env::temp_dir().join(format!("draw-binary-preview-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wide = dir.join("wide.klad");
    let sprite = dir.join("sprite.klad");
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let size = |path: std::path::PathBuf| {
        let bytes = std::fs::read(&path).unwrap();
        let side = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        (side(16), side(20))
    };
    let quick = |bundle: &std::path::Path, name: &str| bundle.join("QuickLook").join(name);
    draw(&[
        "new",
        wide.to_str().unwrap(),
        "--width",
        "1600",
        "--height",
        "900",
        "--background",
        "#f4efe6",
    ]);
    draw(&["new", sprite.to_str().unwrap(), "--pixel", "16x12"]);
    let made = draw(&["preview", wide.to_str().unwrap()]);
    assert_eq!(made.trim(), quick(&wide, "Preview.png").display().to_string());
    assert_eq!(size(quick(&wide, "Preview.png")), (1024, 576), "up to 1024 a side");
    assert_eq!(size(quick(&wide, "Thumbnail.png")), (512, 288), "up to 512 a side");
    let ignored = std::fs::read_to_string(wide.join(".gitignore")).unwrap();
    assert!(ignored.lines().any(|l| l == "QuickLook/"), "{ignored}");
    assert!(
        draw(&["preview", wide.to_str().unwrap(), "--changed"]).starts_with("unchanged"),
        "nothing changed since the last preview"
    );
    let tree: serde_json::Value = serde_json::from_str(&draw(&["ls", wide.to_str().unwrap(), "--json"])).unwrap();
    let layer = tree["layers"][0]["id"].as_str().unwrap().to_string();
    std::fs::write(
        dir.join("ops.jsonl"),
        format!(r##"{{"op":"stroke","layer":"{layer}","path":"M100 100 L900 600","size":24}}"##),
    )
    .unwrap();
    draw(&[
        "apply",
        wide.to_str().unwrap(),
        "--file",
        dir.join("ops.jsonl").to_str().unwrap(),
    ]);
    let all = draw(&["preview", "--all", dir.to_str().unwrap(), "--changed"]);
    assert_eq!(
        all.lines().collect::<Vec<_>>(),
        [
            quick(&sprite, "Preview.png").display().to_string(),
            quick(&wide, "Preview.png").display().to_string()
        ],
        "both: the sprite had none and the wide one changed"
    );
    assert_eq!(
        size(quick(&sprite, "Preview.png")),
        (1024, 768),
        "a pixel drawing grows by whole pixels"
    );
    assert_eq!(size(quick(&sprite, "Thumbnail.png")), (512, 384));
    let missing = run(&["preview", dir.join("none.klad").to_str().unwrap()]);
    assert!(!missing.status.success());
    let _ = std::fs::remove_dir_all(&dir);
}
