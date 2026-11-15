use rune::tok::{self, type_};
use rune::{AppSpec, Color, Cx, El, col, el, row, text};

struct State {
    workspace: String,
}

fn initial_state() -> State {
    let workspace = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    State { workspace }
}

const COMMANDS: &[(&str, &str)] = &[
    (
        "/audit",
        "Audit workspace; add --fix to apply approved fixes",
    ),
    ("/model", "Open model selector and provider auth"),
    ("/paste-image", "Attach image from clipboard"),
    ("/help", "Show available commands"),
];

fn ghost(body: Color, eye: Color) -> El {
    let eye_dot = |x: f32| el().size(8.0, 11.0).radius(4.0).fill(eye).absolute(x, 20.0);
    el().key("ghost")
        .size(64.0, 54.0)
        .radius(20.0)
        .fill(body)
        .child(eye_dot(18.0))
        .child(eye_dot(38.0))
}

fn command_row(name: &str, description: &str, accent: Color, fg: Color) -> El {
    row()
        .gap(tok::space::S3)
        .child(text(name).font(type_::LABEL).color(accent))
        .child(text(description).font(type_::BODY).color(fg))
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    let t = tok::theme(true);
    let accent = tok::accent(1, true);

    let heading = col()
        .center()
        .gap(tok::space::S1)
        .child(
            text("GhostPWN")
                .font(tok::TextStyle::new(30.0, 700.0))
                .color(accent),
        )
        .child(
            text(concat!("v", env!("CARGO_PKG_VERSION")))
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
        .child(
            text("Autonomous penetration testing agent")
                .font(type_::BODY)
                .color(t.text_secondary),
        );

    let workspace = row()
        .gap(tok::space::S2)
        .child(text("workspace").font(type_::LABEL).color(t.text_secondary))
        .child(text(&s.workspace).font(type_::MONO_CAPTION).color(t.text));

    let mut commands = col()
        .gap(tok::space::S2)
        .child(text("Start").font(type_::SECTION).color(t.text_secondary));
    for (name, description) in COMMANDS {
        commands = commands.child(command_row(name, description, accent, t.text));
    }

    col()
        .w_full()
        .h_full()
        .center()
        .gap(tok::space::S6)
        .fill(t.bg)
        .child(ghost(accent, t.bg))
        .child(heading)
        .child(workspace)
        .child(commands)
        .child(
            text("The interactive agent runs in the terminal: `ghostpwn`.")
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
}

fn main() {
    rune::run(AppSpec::new(
        "GhostPWN",
        (760.0, 560.0),
        initial_state,
        view,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn home_view_presents_the_ghost() {
        let env = Env {
            system_dark: true,
            scale: 2.0,
        };
        let mut rt = Runtime::new(initial_state(), view, (760.0, 560.0), env);
        rt.frame(0.0);
        assert!(
            rt.presented("ghost").is_some(),
            "the ghost renders on the home screen"
        );
    }
}
