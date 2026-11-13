//! The home page's project list: one section per owner (Personal, then each
//! team), a row per project with its repositories underneath the name.

use std::rc::Rc;

use gpui_kit::{
  component::v_flex, div, App, InteractiveElement as _, IntoElement, ParentElement as _,
  SharedString, Styled as _,
};

use gpui_kit::base::TestSupportExt as _;

use super::widgets;

use crate::mobile::projects::{Project, Section};

/// What tapping a project does.
pub type OnOpen = Rc<dyn Fn(&Project, &mut App)>;

/// The sections, each a heading over a card of project rows (`project-<id>`).
pub fn sections(sections: &[Section], on_open: OnOpen, cx: &App) -> impl IntoElement {
  v_flex().gap_5().children(sections.iter().map(|section| {
    let rows = section.projects.iter().map(|project| {
      let on_open = on_open.clone();
      let tapped = project.clone();
      widgets::row(
        format!("project-{}", project.id),
        project.name.clone(),
        Some(repos(project)),
        cx,
      )
      .on_click(move |_, _, cx| on_open(&tapped, cx))
    });
    div()
      .id(SharedString::from(format!(
        "projects-section-{}",
        section.title
      )))
      .test_support()
      .child(widgets::section(section.title.clone(), cx).child(widgets::card(cx).children(rows)))
  }))
}

/// The project's repositories by name, as its detail line.
fn repos(project: &Project) -> SharedString {
  project
    .repos
    .iter()
    .map(|r| {
      r.trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(r)
    })
    .collect::<Vec<_>>()
    .join(", ")
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;
  use gpui_kit::test::TestWindowExt as _;
  use gpui_kit::{
    div, px, size, AppContext as _, Bounds, Context, Point, Render, TestAppContext, Window,
    WindowBounds, WindowOptions,
  };
  use std::cell::RefCell;

  struct Host {
    sections: Vec<Section>,
    opened: Rc<RefCell<Vec<String>>>,
  }

  impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
      let opened = self.opened.clone();
      let on_open: OnOpen = Rc::new(move |p, _| opened.borrow_mut().push(p.id.clone()));
      div()
        .size_full()
        .child(sections(&self.sections, on_open, cx))
    }
  }

  fn project(id: &str, org: Option<&str>) -> Project {
    Project {
      id: id.into(),
      name: format!("P {id}"),
      org_id: org.map(Into::into),
      repos: vec!["acme/web".into()],
    }
  }

  #[gpui_kit::test]
  fn draws_each_section_and_opens_the_project_tapped(cx: &mut TestAppContext) {
    cx.update(super::super::theme::init);
    let opened = Rc::new(RefCell::new(Vec::new()));
    let host_opened = opened.clone();
    let (handle, _) = cx.update(|cx| {
      gpui_kit::open_window(
        WindowOptions {
          window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(412.), px(892.)),
          })),
          ..Default::default()
        },
        cx,
        |_, cx| {
          cx.new(|_| Host {
            sections: vec![
              Section {
                title: "Personal".into(),
                projects: vec![project("p", None)],
              },
              Section {
                title: "Acme".into(),
                projects: vec![project("a", Some("o1"))],
              },
            ],
            opened: host_opened,
          })
        },
      )
      .expect("open test window")
    });
    cx.update_window(handle, |_, window, cx| {
      window.render_frame(cx);
      assert!(window.try_find("projects-section-Personal").is_some());
      assert!(window.try_find("projects-section-Acme").is_some());
      assert!(window.try_find("project-p").is_some());
      window.click("project-a", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(*opened.borrow(), ["a"]);
  }
}
