target_dir := env("KLAD_TARGET_DIR", if os() == "macos" { "target" } else { "target/" + os() })
klad := target_dir / "release" / "klad" + if os() == "windows" { ".exe" } else { "" }
version := `sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1`

export CARGO_TARGET_DIR := target_dir
export KLAD_TARGET_DIR := target_dir

default: compile

build part="patch":
    bash packaging/bump.sh {{part}}
    just compile

compile:
    cargo build --release -p klad-cli
    {{klad}} --version

test:
    cargo test --workspace --no-fail-fast
    python3 docs/skill/drawkit.py --check

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

samples: compile
    for s in lantern ink-shapes bounce creature courier solids lemons ruin sign tree meadow paintover rig sprout ground lit-lantern lit-creature; do FORCE=1 bash samples/$s/build.sh >/dev/null || { echo "samples/$s failed" >&2; exit 1; }; echo "samples/$s"; done

app:
    bash packaging/macos/app.sh

publish release=version:
    bash packaging/publish.sh {{release}} macos dist/klad-{{version}}-macos.dmg

dist target=`rustc -vV | sed -n 's/^host: //p'`:
    bash packaging/archive.sh {{target}}

icon: compile
    KLAD={{klad}} bash packaging/icon/build.sh

hello: compile
    KLAD={{klad}} bash packaging/icon/hello.sh "$(mktemp -d)"

formats version: compile
    bash packaging/formats.sh {{klad}} {{version}}

reference: compile
    {{klad}} reference > docs/cli.md

notices:
    bash licenses/notices.sh

notices-check:
    bash licenses/notices.sh "${TMPDIR:-/tmp}/draw-notices.md"
    diff -q "${TMPDIR:-/tmp}/draw-notices.md" THIRD-PARTY-NOTICES.md

audit: notices-check
    @cargo deny --version >/dev/null 2>&1 || { echo "cargo-deny is not installed: cargo install cargo-deny --locked" >&2; exit 1; }
    cargo deny check

fmt:
    cargo fmt --all

bench *args: compile
    {{klad}} bench {{args}}

demo bundle="scratch/demo.klad": compile
    {{klad}} demo {{bundle}} --force
    {{klad}} render {{bundle}}

hash bundle="scratch/demo.klad": compile
    {{klad}} hash {{bundle}} --no-cache

open bundle="scratch/pen.klad": compile
    {{klad}} open {{bundle}}

stress bundle="scratch/stress.klad": compile
    {{klad}} stress {{bundle}}

ui-pictures:
    cargo test -p klad-app -- --ignored panel_pictures

ui-time:
    cargo test --release -p klad-app -- --ignored ui_time_per_frame --nocapture

drag-time bundle="":
    KLAD_DRAG_BUNDLE={{bundle}} cargo test --release -p klad-app -- --ignored drag_time --nocapture

journeys out="":
    cargo build -p klad-cli
    KLAD_JOURNEY_OUT={{out}} cargo test -p klad-app -- journeys

notes version:
    @bash packaging/notes.sh {{version}}

memory sizes="4096 8192 16384": compile
    @bash packaging/memory.sh {{klad}} "{{sizes}}"

monkey out="":
    cargo build -p klad-cli
    KLAD_STAMP="$(date '+%Y-%m-%d %H:%M')" KLAD_MONKEY_OUT={{out}} cargo test -p klad-app -- --ignored monkey::random_use --nocapture

sizes out="":
    KLAD_JOURNEY_OUT={{out}} cargo test -p klad-app -- --ignored sizes::

sweep out="":
    cargo build -p klad-cli
    KLAD_SWEEP_OUT={{out}} cargo test -p klad-app -- --include-ignored sweep::

inventory out=(justfile_directory() / "scratch"):
    KLAD_INVENTORY_OUT={{out}}/inventory.md cargo test -p klad-app -- --ignored command_inventory
    KLAD_INVENTORY_OUT={{out}}/controls.md cargo test -p klad-app -- --ignored editor_inventory
