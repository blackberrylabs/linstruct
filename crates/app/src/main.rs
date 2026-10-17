//! Linstruct desktop app (iced, on bbl-ui): what each host loads (with its budget), the
//! findings most serious first, the detail of the selected one with its source. Reads the
//! same core as the CLI; writes only its preferences (`~/.config/linstruct/settings.json`).
#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    button, checkbox, column, container, horizontal_space, mouse_area, progress_bar, row,
    scrollable, text, text_input, Space,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Task, Theme};

use bbl_report::Severity;
use bbl_ui::palette::{alert, amber, good, muted};
use bbl_ui::widgets::{card, inner, kpi, pill, small_button, swatch};
use bbl_ui::{Lang, Prefs, Strings};
use linstruct_core::model::{Finding, HostStatus, Inventory, Kind, SlotState, Stack};
use linstruct_core::scan::{display_home, rel_plain};

const EN: &str = include_str!("../../../locales/en.json");
const FR: &str = include_str!("../../../locales/fr.json");
/// Slots shown per host card before "+ n more".
const SLOTS_SHOWN: usize = 6;

fn main() -> iced::Result {
    iced::application(App::title, App::update, App::view)
        .theme(App::theme)
        .window_size((1280.0, 820.0))
        .run_with(App::new)
}

#[derive(Debug, Clone)]
enum Message {
    Loaded(Arc<Inventory>, u64),
    Rescan,
    SetLang(Lang),
    ToggleTheme,
    Select(String),
    FilterHost(Option<String>),
    FilterProject(Option<PathBuf>),
    ToggleInfo(bool),
    ToggleWhatIf(bool),
    ExpandHost(String),
    Search(String),
    OpenFile(String),
    OpenUrl(String),
    Copy(String),
}

struct App {
    prefs: Prefs<()>,
    lang: Lang,
    strings: Strings,
    dark: bool,
    home: PathBuf,
    roots: Vec<PathBuf>,
    inventory: Option<Arc<Inventory>>,
    loading: bool,
    elapsed_ms: u64,
    selected: Option<String>,
    host_filter: Option<String>,
    project_filter: Option<PathBuf>,
    show_info: bool,
    show_what_if: bool,
    expanded: HashSet<String>,
    search: String,
}

impl App {
    fn new() -> (App, Task<Message>) {
        let prefs: Prefs<()> = Prefs::load("linstruct");
        let lang = prefs.initial_lang();
        let dark = prefs.initial_dark();
        let strings = Strings::load(EN, FR, lang);
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        // `--select <finding id>` (the Blackberrylabs tower) preselects a finding; other
        // arguments are project roots.
        let (selected, roots) = parse_args();
        let task = load(home.clone(), roots.clone());
        (
            App {
                prefs,
                lang,
                strings,
                dark,
                home,
                roots,
                inventory: None,
                loading: true,
                elapsed_ms: 0,
                selected,
                host_filter: None,
                project_filter: None,
                show_info: false,
                show_what_if: false,
                expanded: HashSet::new(),
                search: String::new(),
            },
            task,
        )
    }

    fn title(&self) -> String {
        self.strings.t("app.title", &[])
    }

    fn theme(&self) -> Theme {
        if self.dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

    fn remember(&mut self) {
        self.prefs.remember(self.lang, self.dark);
        self.prefs.save();
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(inv, ms) => {
                // a preselected finding of an absent host unfolds the "what if" section
                if let Some(id) = &self.selected {
                    if let Some(f) = inv.findings.iter().find(|f| &f.id == id) {
                        if !f.present {
                            self.show_what_if = true;
                        }
                        if f.kind.severity() == Severity::Info {
                            self.show_info = true;
                        }
                    }
                }
                self.inventory = Some(inv);
                self.elapsed_ms = ms;
                self.loading = false;
            }
            Message::Rescan => {
                self.loading = true;
                return load(self.home.clone(), self.roots.clone());
            }
            Message::SetLang(l) => {
                self.lang = l;
                self.strings = Strings::load(EN, FR, l);
                self.remember();
            }
            Message::ToggleTheme => {
                self.dark = !self.dark;
                self.remember();
            }
            Message::Select(id) => self.selected = Some(id),
            Message::FilterHost(h) => self.host_filter = h,
            Message::FilterProject(p) => self.project_filter = p,
            Message::ToggleInfo(b) => self.show_info = b,
            Message::ToggleWhatIf(b) => {
                self.show_what_if = b;
                if !b {
                    if let Some(h) = &self.host_filter {
                        if self
                            .inventory
                            .as_ref()
                            .is_some_and(|i| i.hosts.iter().any(|x| &x.id == h && !x.present))
                        {
                            self.host_filter = None;
                        }
                    }
                }
            }
            Message::ExpandHost(id) => {
                if !self.expanded.remove(&id) {
                    self.expanded.insert(id);
                }
            }
            Message::Search(s) => self.search = s,
            Message::OpenFile(p) | Message::OpenUrl(p) => {
                // the one thing the app launches: the system opener, on a path or a URL —
                // never a command of ours
                let target = p
                    .rsplit_once(':')
                    .filter(|(_, l)| {
                        l.chars().all(|c| c.is_ascii_digit()) && !p.starts_with("http")
                    })
                    .map(|(a, _)| a.to_string())
                    .unwrap_or(p);
                let _ = std::process::Command::new("xdg-open").arg(&target).spawn();
            }
            Message::Copy(s) => return iced::clipboard::write(s),
        }
        Task::none()
    }

    // ---- helpers ---------------------------------------------------------------------

    fn sev_color(&self, s: Severity) -> Color {
        match s {
            Severity::ActNow => alert(self.dark),
            Severity::Review => amber(self.dark),
            Severity::GoodToKnow => good(self.dark),
            Severity::Info => muted(),
        }
    }

    /// The findings the list shows, in order.
    fn visible(&self) -> Vec<&Finding> {
        let Some(inv) = self.inventory.as_deref() else {
            return Vec::new();
        };
        let q = self.search.to_lowercase();
        inv.findings
            .iter()
            .filter(|f| f.present || self.show_what_if)
            .filter(|f| self.host_filter.as_ref().is_none_or(|h| &f.host == h))
            .filter(|f| {
                self.project_filter
                    .as_ref()
                    .is_none_or(|p| f.path.as_ref().is_some_and(|x| x.starts_with(p)))
            })
            .filter(|f| {
                q.is_empty()
                    || f.title.to_lowercase().contains(&q)
                    || f.subject.to_lowercase().contains(&q)
                    || f.host_name.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn selected_finding(&self) -> Option<&Finding> {
        let inv = self.inventory.as_deref()?;
        let id = self.selected.as_deref()?;
        inv.findings.iter().find(|f| f.id == id)
    }

    fn present_counts(&self, inv: &Inventory) -> [usize; 4] {
        let mut c = [0usize; 4];
        for f in inv.present_findings() {
            c[(f.kind.severity().number() - 1) as usize] += 1;
        }
        c
    }

    // ---- view ------------------------------------------------------------------------

    fn view(&self) -> Element<'_, Message> {
        let s = &self.strings;
        let inv = self.inventory.as_deref();
        let body: Element<'_, Message> = match inv {
            Some(i) => row![self.view_hosts(i), self.view_list(i), self.view_detail(i)]
                .spacing(12)
                .height(Length::Fill)
                .into(),
            None => container(
                text(s.t("header.scanning", &[]))
                    .size(16)
                    .style(text::secondary),
            )
            .center(Length::Fill)
            .into(),
        };
        let status = match inv {
            Some(i) => s.t(
                "status.ready",
                &[
                    (
                        "files",
                        &i.files
                            .iter()
                            .filter(|f| f.error.is_none())
                            .count()
                            .to_string(),
                    ),
                    (
                        "hosts",
                        &i.hosts.iter().filter(|h| h.present).count().to_string(),
                    ),
                    ("findings", &i.present_findings().count().to_string()),
                ],
            ),
            None => s.t("header.scanning", &[]),
        };
        let theme_label = if self.dark {
            s.t("theme.light", &[])
        } else {
            s.t("theme.dark", &[])
        };
        let footer = bbl_ui::widgets::footer(
            status,
            self.lang,
            Message::SetLang,
            theme_label,
            Message::ToggleTheme,
        );
        let mut col = column![self.view_header(inv)].spacing(12).padding(14);
        if let Some(i) = inv {
            col = col.push(self.view_kpis(i));
        }
        col.push(body).push(footer).into()
    }

    fn view_header(&self, inv: Option<&Inventory>) -> Element<'_, Message> {
        let s = &self.strings;
        let sentence: Element<'_, Message> = match inv {
            None => text(s.t("headline.scanning", &[])).size(20).into(),
            Some(i) => {
                let hosts = i.hosts.iter().filter(|h| h.present).count();
                let [act, review, _, _] = self.present_counts(i);
                let things = |n: usize| {
                    if n == 1 {
                        s.t("headline.things_one", &[])
                    } else {
                        s.t("headline.things", &[("n", &n.to_string())])
                    }
                };
                let hosts_s = if hosts == 1 {
                    s.t("headline.hosts_one", &[])
                } else {
                    s.t("headline.hosts", &[("n", &hosts.to_string())])
                };
                if hosts == 0 {
                    text(s.t("headline.no_host", &[])).size(20).into()
                } else if act == 0 && review == 0 {
                    text(s.t("headline.clear", &[("hosts", &hosts_s)]))
                        .size(20)
                        .into()
                } else if act == 0 {
                    text(s.t(
                        "headline.review",
                        &[("hosts", &hosts_s), ("review", &review.to_string())],
                    ))
                    .size(20)
                    .into()
                } else {
                    let full = s.t(
                        "headline.act",
                        &[
                            ("hosts", &hosts_s),
                            ("act", &things(act)),
                            ("review", &review.to_string()),
                        ],
                    );
                    let marker = things(act);
                    match full.find(&marker) {
                        Some(i) => {
                            let (a, rest) = full.split_at(i);
                            let (b, c) = rest.split_at(marker.len());
                            row![
                                text(a.to_string()).size(20),
                                text(b.to_string()).size(20).color(alert(self.dark)),
                                text(c.to_string()).size(20)
                            ]
                            .into()
                        }
                        None => text(full).size(20).into(),
                    }
                }
            }
        };
        let mut chips = row![].spacing(4).align_y(Alignment::Center);
        if let Some(i) = inv {
            if i.projects.len() > 1 {
                chips = chips.push(chip(
                    s.t("list.all", &[]),
                    self.project_filter.is_none(),
                    Message::FilterProject(None),
                ));
                for p in i.projects.iter().take(4) {
                    chips = chips.push(chip(
                        display_home(p, &i.home),
                        self.project_filter.as_ref() == Some(p),
                        Message::FilterProject(Some(p.clone())),
                    ));
                }
                if i.projects.len() > 4 {
                    chips = chips.push(
                        text(s.t(
                            "header.more_projects",
                            &[("n", &(i.projects.len() - 4).to_string())],
                        ))
                        .size(11)
                        .style(text::secondary),
                    );
                }
            }
        }
        let when = if self.loading {
            s.t("header.scanning", &[])
        } else {
            s.t(
                "header.read",
                &[("s", &format!("{:.1}", self.elapsed_ms as f64 / 1000.0))],
            )
        };
        let rescan: Element<'_, Message> = if self.loading {
            button(text(s.t("header.scanning", &[])).size(12))
                .style(button::secondary)
                .into()
        } else {
            button(text(s.t("header.rescan", &[])).size(12))
                .style(button::primary)
                .on_press(Message::Rescan)
                .into()
        };
        row![
            sentence,
            horizontal_space(),
            chips,
            text(when).size(12).style(text::secondary),
            rescan
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    fn view_kpis(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let files = inv.files.iter().filter(|f| f.error.is_none()).count();
        let user_files = inv
            .files
            .iter()
            .filter(|f| f.error.is_none() && f.project.is_none())
            .count();
        let present: Vec<&HostStatus> = inv.hosts.iter().filter(|h| h.present).collect();
        let absent = inv.hosts.len() - present.len();
        let heaviest = present.iter().max_by_key(|h| h.tokens_est);
        let [act, review, know, info] = self.present_counts(inv);
        let n = act + review + know + info;
        let names: Vec<&str> = present.iter().map(|h| h.name.as_str()).collect();
        row![
            kpi(
                s.t("kpi.files", &[]),
                files.to_string(),
                s.t(
                    "kpi.files.sub",
                    &[
                        ("projects", &inv.projects.len().to_string()),
                        ("user", &user_files.to_string())
                    ]
                ),
                None
            ),
            kpi(
                s.t("kpi.hosts", &[]),
                present.len().to_string(),
                if names.is_empty() {
                    s.t("kpi.hosts.none", &[])
                } else {
                    s.t(
                        "kpi.hosts.sub",
                        &[
                            ("list", &names.join(" · ")),
                            ("absent", &absent.to_string()),
                        ],
                    )
                },
                None
            ),
            kpi(
                s.t("kpi.heaviest", &[]),
                heaviest
                    .map(|h| format!("{} ≈ {}", h.name, thousands(h.tokens_est)))
                    .unwrap_or_else(|| "—".into()),
                s.t("kpi.heaviest.sub", &[]),
                heaviest.map(|h| if h.tokens_est > 20_000 {
                    amber(self.dark)
                } else {
                    good(self.dark)
                })
            ),
            kpi(
                s.t("kpi.findings", &[]),
                n.to_string(),
                s.t(
                    "kpi.findings.sub",
                    &[
                        ("act", &act.to_string()),
                        ("review", &review.to_string()),
                        ("know", &know.to_string()),
                        ("info", &info.to_string())
                    ]
                ),
                Some(if act > 0 {
                    alert(self.dark)
                } else if review > 0 {
                    amber(self.dark)
                } else {
                    good(self.dark)
                })
            ),
        ]
        .spacing(12)
        .height(Length::Fixed(96.0))
        .into()
    }

    // ---- hosts column ------------------------------------------------------------------

    fn view_hosts(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let mut col = column![].spacing(8);
        let present: Vec<&HostStatus> = inv.hosts.iter().filter(|h| h.present).collect();
        if present.is_empty() {
            col = col.push(text(s.t("hosts.none", &[])).size(13).style(text::secondary));
        }
        for h in &present {
            col = col.push(self.view_host(inv, h));
        }
        let absent: Vec<&HostStatus> = inv.hosts.iter().filter(|h| !h.present).collect();
        if !absent.is_empty() {
            let what_if = inv.findings.iter().filter(|f| !f.present).count();
            let label = s.t(
                if self.show_what_if {
                    "hosts.what_if_open"
                } else {
                    "hosts.what_if"
                },
                &[
                    ("n", &absent.len().to_string()),
                    ("findings", &what_if.to_string()),
                ],
            );
            let toggle = container(text(label).size(12).style(text::secondary))
                .padding([6, 8])
                .width(Length::Fill)
                .style(|theme: &Theme| {
                    let p = theme.extended_palette();
                    container::Style {
                        border: Border {
                            radius: 8.0.into(),
                            width: 1.0,
                            color: p.background.strong.color,
                        },
                        ..Default::default()
                    }
                });
            col = col.push(mouse_area(toggle).on_press(Message::ToggleWhatIf(!self.show_what_if)));
            if self.show_what_if {
                for h in &absent {
                    col = col.push(self.view_host(inv, h));
                }
            }
        }
        card(
            column![
                text(s.t("hosts.title", &[])).size(15),
                text(s.t("hosts.subtitle", &[]))
                    .size(12)
                    .style(text::secondary),
                scrollable(col.padding(iced::Padding {
                    right: 14.0,
                    ..Default::default()
                }))
                .height(Length::Fill),
            ]
            .spacing(8)
            .into(),
        )
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .into()
    }

    fn view_host(&self, inv: &Inventory, h: &HostStatus) -> Element<'_, Message> {
        let s = &self.strings;
        let stacks: Vec<&Stack> = inv
            .stacks
            .iter()
            .filter(|st| {
                st.host == h.id
                    && self
                        .project_filter
                        .as_ref()
                        .is_none_or(|p| st.project.is_none() || st.project.as_ref() == Some(p))
            })
            .collect();
        let bytes: usize = stacks.iter().map(|st| st.bytes).sum();
        let tokens: usize = stacks.iter().map(|st| st.tokens_est).sum();
        let budget = stacks
            .iter()
            .filter_map(|st| st.budget.as_ref())
            .max_by_key(|b| b.used * 1000 / b.amount.max(1));
        let on = self.host_filter.as_deref() == Some(h.id.as_str());
        let name: Element<'_, Message> = if h.present {
            text(h.name.clone()).size(14).into()
        } else {
            text(h.name.clone()).size(14).style(text::secondary).into()
        };
        let presence = if h.present {
            h.presence.clone()
        } else {
            s.t("host.absent", &[])
        };
        let totals = match budget {
            Some(b) => {
                let over = b.used >= b.amount;
                let label = s.t(
                    "host.budget",
                    &[
                        ("used", &thousands(b.used)),
                        ("amount", &thousands(b.amount)),
                        ("unit", &b.unit),
                    ],
                );
                if over {
                    row![
                        text(label).size(11).style(text::secondary),
                        text(s.t(&format!("host.overflow.{}", b.on_overflow), &[]))
                            .size(11)
                            .color(alert(self.dark))
                    ]
                    .spacing(4)
                } else {
                    row![text(label).size(11).style(text::secondary)]
                }
            }
            None => row![text(s.t(
                "host.totals",
                &[("bytes", &thousands(bytes)), ("tokens", &thousands(tokens))]
            ))
            .size(11)
            .style(text::secondary)],
        };
        let mut col = column![row![
            name,
            text(presence).size(11).style(text::secondary),
            horizontal_space(),
            totals
        ]
        .spacing(8)
        .align_y(Alignment::Center)]
        .spacing(4);
        if let Some(b) = budget {
            let ratio = (b.used as f32 / b.amount.max(1) as f32).min(1.0);
            let color = if b.used >= b.amount {
                alert(self.dark)
            } else if ratio >= 0.8 {
                amber(self.dark)
            } else {
                good(self.dark)
            };
            col = col.push(
                progress_bar(0.0..=1.0, ratio)
                    .height(Length::Fixed(6.0))
                    .style(move |theme: &Theme| {
                        let p = theme.extended_palette();
                        progress_bar::Style {
                            background: Background::Color(p.background.strong.color),
                            bar: Background::Color(color),
                            border: Border {
                                radius: 3.0.into(),
                                ..Default::default()
                            },
                        }
                    }),
            );
        }
        // the slots: user level first, then each project
        let mut slots: Vec<(&Stack, &linstruct_core::model::Slot)> = Vec::new();
        for st in &stacks {
            for sl in &st.slots {
                slots.push((st, sl));
            }
        }
        let expanded = self.expanded.contains(&h.id);
        let shown = if expanded {
            slots.len()
        } else {
            slots.len().min(SLOTS_SHOWN)
        };
        for (st, sl) in slots.iter().take(shown) {
            let f = &inv.files[sl.file];
            let path = rel_plain(&f.path, st.project.as_deref(), &inv.home);
            let (state, color) = self.slot_state(sl, f);
            let state_el: Element<'_, Message> = match &sl.state {
                SlotState::Loaded => text(state).size(11).style(text::secondary).into(),
                SlotState::OnDemand => text(state).size(11).color(color).into(),
                _ => pill(state, color),
            };
            let r = row![
                text(sl.role.clone())
                    .size(10)
                    .style(text::secondary)
                    .width(Length::Fixed(44.0)),
                text(path).size(11).font(iced::Font::MONOSPACE),
                horizontal_space(),
                text(format!("{} B", thousands(f.bytes)))
                    .size(11)
                    .style(text::secondary),
                state_el,
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            col = col.push(r);
        }
        if slots.len() > shown || expanded && slots.len() > SLOTS_SHOWN {
            let label = if expanded {
                s.t("host.less", &[])
            } else {
                s.t("host.more", &[("n", &(slots.len() - shown).to_string())])
            };
            col = col.push(small_button(label, Message::ExpandHost(h.id.clone())));
        }
        if slots.is_empty() {
            col = col.push(
                text(s.t("host.no_files", &[]))
                    .size(11)
                    .style(text::secondary),
            );
        }
        let cell = container(col)
            .padding(8)
            .width(Length::Fill)
            .style(move |theme: &Theme| {
                let p = theme.extended_palette();
                container::Style {
                    background: if on {
                        Some(Background::Color(p.primary.weak.color))
                    } else {
                        None
                    },
                    border: Border {
                        radius: 8.0.into(),
                        width: 1.0,
                        color: if on {
                            p.primary.base.color
                        } else {
                            p.background.strong.color
                        },
                    },
                    ..Default::default()
                }
            });
        let id = h.id.clone();
        mouse_area(cell)
            .on_press(Message::FilterHost(if on { None } else { Some(id) }))
            .into()
    }

    fn slot_state(
        &self,
        sl: &linstruct_core::model::Slot,
        f: &linstruct_core::model::InstrFile,
    ) -> (String, Color) {
        let s = &self.strings;
        match &sl.state {
            SlotState::Loaded => (
                if f.lines > 0 {
                    s.t("slot.loaded_lines", &[("n", &f.lines.to_string())])
                } else {
                    s.t("slot.loaded", &[])
                },
                good(self.dark),
            ),
            SlotState::OnDemand => (
                s.t(&format!("slot.when.{}", sl.when), &[]),
                amber(self.dark),
            ),
            SlotState::Cut { at_line, .. } => (
                s.t("slot.cut", &[("line", &at_line.to_string())]),
                alert(self.dark),
            ),
            SlotState::Skipped { reason } => {
                (s.t("slot.skipped", &[("reason", reason)]), amber(self.dark))
            }
            SlotState::Shadowed { by } => (s.t("slot.shadowed", &[("by", by)]), amber(self.dark)),
            SlotState::Ignored { .. } => (s.t("slot.ignored", &[]), alert(self.dark)),
        }
    }

    // ---- findings column -----------------------------------------------------------------

    fn view_list(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let all = self.visible();
        let hidden_info = all
            .iter()
            .filter(|f| f.kind.severity() == Severity::Info)
            .count();
        let lines: Vec<&Finding> = all
            .into_iter()
            .filter(|f| self.show_info || f.kind.severity() != Severity::Info)
            .collect();
        let mut chips = row![].spacing(4);
        chips = chips.push(chip(
            s.t("list.all_hosts", &[]),
            self.host_filter.is_none(),
            Message::FilterHost(None),
        ));
        for h in inv.hosts.iter().filter(|h| h.present || self.show_what_if) {
            if h.findings == 0 {
                continue;
            }
            chips = chips.push(chip(
                h.name.clone(),
                self.host_filter.as_deref() == Some(&h.id),
                Message::FilterHost(Some(h.id.clone())),
            ));
        }
        let filters = column![
            scrollable(chips).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(0).scroller_width(0)
            )),
            row![
                text_input(&s.t("list.search", &[]), &self.search)
                    .on_input(Message::Search)
                    .size(12)
                    .padding(5)
                    .width(Length::FillPortion(2)),
                checkbox(s.t("list.show_info", &[]), self.show_info)
                    .on_toggle(Message::ToggleInfo)
                    .size(14)
                    .text_size(12),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(6);
        let body: Element<'_, Message> = if lines.is_empty() {
            let msg = if inv.present_findings().count() == 0
                && self.host_filter.is_none()
                && self.search.is_empty()
            {
                s.t("list.empty_clear", &[])
            } else {
                s.t("list.empty_filter", &[])
            };
            let mut c = column![text(msg).size(13).style(text::secondary)].spacing(6);
            if hidden_info > 0 {
                c = c.push(
                    text(s.t("list.more_hidden", &[("n", &hidden_info.to_string())]))
                        .size(12)
                        .style(text::secondary),
                );
            }
            c.into()
        } else {
            let mut rows = column![].spacing(2);
            let mut current: Option<Severity> = None;
            for f in &lines {
                let sev = f.kind.severity();
                if current != Some(sev) {
                    current = Some(sev);
                    let n = lines.iter().filter(|x| x.kind.severity() == sev).count();
                    rows = rows.push(
                        text(s.t(
                            "list.group",
                            &[
                                (
                                    "sev",
                                    &s.t(&format!("sev.{}", sev.number()), &[]).to_uppercase(),
                                ),
                                ("n", &n.to_string()),
                            ],
                        ))
                        .size(11)
                        .style(text::secondary),
                    );
                }
                rows = rows.push(self.view_row(f));
            }
            if hidden_info > 0 && !self.show_info {
                rows = rows.push(
                    text(s.t("list.more_hidden", &[("n", &hidden_info.to_string())]))
                        .size(12)
                        .style(text::secondary),
                );
            }
            scrollable(rows.padding(iced::Padding {
                right: 14.0,
                ..Default::default()
            }))
            .height(Length::Fill)
            .into()
        };
        card(
            column![
                text(s.t("list.title", &[])).size(15),
                text(s.t("list.subtitle", &[]))
                    .size(12)
                    .style(text::secondary),
                filters,
                body
            ]
            .spacing(8)
            .into(),
        )
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .into()
    }

    fn view_row(&self, f: &Finding) -> Element<'_, Message> {
        let selected = self.selected.as_deref() == Some(f.id.as_str());
        let color = self.sev_color(f.kind.severity());
        let host: Element<'_, Message> = if selected {
            text(short_host(&f.host_name))
                .size(11)
                .width(Length::Fixed(74.0))
                .into()
        } else {
            text(short_host(&f.host_name))
                .size(11)
                .style(text::secondary)
                .width(Length::Fixed(74.0))
                .into()
        };
        let mut title = text(plain(&f.title)).size(13);
        if !f.present {
            title = title.style(text::secondary);
        }
        let r = row![
            swatch(color, 9),
            host,
            title,
            horizontal_space(),
            text("›").size(13).style(text::secondary)
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let cell = container(r)
            .padding([6, 8])
            .width(Length::Fill)
            .style(move |theme: &Theme| {
                let p = theme.extended_palette();
                container::Style {
                    background: if selected {
                        Some(Background::Color(p.primary.weak.color))
                    } else {
                        None
                    },
                    border: Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });
        mouse_area(cell)
            .on_press(Message::Select(f.id.clone()))
            .into()
    }

    // ---- detail column -------------------------------------------------------------------

    fn view_detail(&self, inv: &Inventory) -> Element<'_, Message> {
        let content: Element<'_, Message> = match self.selected_finding() {
            Some(f) => self.view_finding(inv, f),
            None => self.view_about(inv),
        };
        card(
            scrollable(container(content).padding(iced::Padding {
                right: 14.0,
                ..Default::default()
            }))
            .height(Length::Fill)
            .into(),
        )
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .into()
    }

    fn view_finding(&self, inv: &Inventory, f: &Finding) -> Element<'_, Message> {
        let s = &self.strings;
        let sev = f.kind.severity();
        let heading = |k: &str| {
            text(s.t(k, &[]).to_uppercase())
                .size(11)
                .style(text::secondary)
        };
        let mut head = row![
            pill(f.kind.label().to_string(), self.sev_color(sev)),
            text(s.t(&format!("sev.{}", sev.number()), &[]))
                .size(12)
                .style(text::secondary),
            text("·").size(12).style(text::secondary),
            text(f.host_name.clone()).size(12).style(text::secondary)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if !f.present {
            head = head.push(pill(s.t("detail.what_if", &[]), muted()));
        }
        let mut col = column![head, text(plain(&f.title)).size(16)].spacing(4);
        col = col.push(Space::with_height(8));
        col = col.push(heading("detail.means"));
        col = col.push(
            text(format!(
                "{} {}",
                plain(&f.detail),
                s.t(&format!("kind.{}", f.kind.slug()), &[])
            ))
            .size(13),
        );
        if let Some(p) = &f.path {
            col = col.push(Space::with_height(8));
            col = col.push(heading("detail.where"));
            let shown = match f.line {
                Some(l) => format!("{}:{l}", display_home(p, &inv.home)),
                None => display_home(p, &inv.home),
            };
            let target = p.display().to_string();
            col = col.push(inner(
                column![
                    text(shown).size(12).font(iced::Font::MONOSPACE),
                    row![
                        small_button(
                            s.t("detail.open_file", &[]),
                            Message::OpenFile(target.clone())
                        ),
                        small_button(s.t("detail.copy_path", &[]), Message::Copy(target))
                    ]
                    .spacing(6),
                ]
                .spacing(4)
                .into(),
            ));
        }
        col = col.push(Space::with_height(8));
        col = col.push(heading("detail.do"));
        if let Some(h) = &f.hint {
            col = col.push(
                row![
                    text("→").size(13).style(text::secondary),
                    text(plain(h)).size(13)
                ]
                .spacing(8),
            );
        }
        if let Some(sheet) = linstruct_core::rules::embedded().host(&f.host) {
            if let Some(show) = sheet.hint("show") {
                col = col.push(
                    row![
                        text("→").size(13).style(text::secondary),
                        column![
                            text(s.t("detail.check_in", &[("host", &f.host_name)])).size(13),
                            row![
                                text(show.to_string()).size(12).font(iced::Font::MONOSPACE),
                                small_button(
                                    s.t("detail.copy_command", &[]),
                                    Message::Copy(show.to_string())
                                )
                            ]
                            .spacing(8)
                            .align_y(Alignment::Center)
                        ]
                        .spacing(3)
                    ]
                    .spacing(8),
                );
            }
        }
        if let Some(u) = &f.source_url {
            col = col.push(Space::with_height(8));
            col = col.push(heading("detail.source"));
            let kind = if u.contains("github.com") && u.contains("/blob/") {
                s.t("source.code", &[])
            } else if u.contains("/issues/") || u.contains("forum.") {
                s.t("source.observed", &[])
            } else {
                s.t("source.docs", &[])
            };
            col = col.push(
                column![
                    text(kind).size(12).style(text::secondary),
                    text(shorten(u, 56)).size(11).font(iced::Font::MONOSPACE),
                    row![
                        small_button(s.t("detail.open_link", &[]), Message::OpenUrl(u.clone())),
                        small_button(s.t("detail.copy_path", &[]), Message::Copy(u.clone()))
                    ]
                    .spacing(6),
                ]
                .spacing(3),
            );
        }
        col = col.push(Space::with_height(12));
        col = col.push(
            text(s.t("detail.note", &[]))
                .size(12)
                .style(text::secondary),
        );
        col.into()
    }

    fn view_about(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let mut col = column![
            text(s.t("about.title", &[])).size(15),
            text(s.t("about.body", &[])).size(13)
        ]
        .spacing(8);
        col = col.push(Space::with_height(6));
        col = col.push(
            text(s.t("about.severities", &[]).to_uppercase())
                .size(11)
                .style(text::secondary),
        );
        for sev in Severity::ALL {
            let kinds: Vec<&str> = Kind::ALL
                .iter()
                .filter(|k| k.severity() == sev)
                .map(|k| k.label())
                .collect();
            col = col.push(
                row![
                    swatch(self.sev_color(sev), 9),
                    text(s.t(&format!("sev.{}", sev.number()), &[])).size(13),
                    text(kinds.join(" · ").to_lowercase())
                        .size(12)
                        .style(text::secondary)
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        if !inv.not_read.is_empty() {
            col = col.push(Space::with_height(6));
            col = col.push(
                text(s.t("about.not_read", &[]).to_uppercase())
                    .size(11)
                    .style(text::secondary),
            );
            for n in &inv.not_read {
                col = col.push(
                    text(format!("{} — {}", n.name, n.reason))
                        .size(12)
                        .style(text::secondary),
                );
            }
        }
        if !inv.notes.is_empty() {
            col = col.push(Space::with_height(6));
            col = col.push(
                text(s.t("about.notes", &[]).to_uppercase())
                    .size(11)
                    .style(text::secondary),
            );
            for n in &inv.notes {
                col = col.push(text(n.clone()).size(12).style(text::secondary));
            }
        }
        col = col.push(Space::with_height(12));
        col = col.push(
            text(s.t("detail.note", &[]))
                .size(12)
                .style(text::secondary),
        );
        col.into()
    }
}

fn chip<'a>(label: String, on: bool, msg: Message) -> Element<'a, Message> {
    let b = button(text(label).size(11)).padding([2, 8]);
    if on {
        b.style(button::primary).on_press(msg).into()
    } else {
        b.style(button::secondary).on_press(msg).into()
    }
}

/// `--select <finding id>` and the project roots given as plain arguments.
fn parse_args() -> (Option<String>, Vec<PathBuf>) {
    let mut selected = None;
    let mut roots = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--select" {
            selected = args.next();
        } else if let Some(v) = a.strip_prefix("--select=") {
            selected = Some(v.to_string());
        } else if !a.starts_with('-') {
            roots.push(PathBuf::from(a));
        }
    }
    (selected, roots)
}

/// The scan on a thread, so the window stays responsive.
fn load(home: PathBuf, roots: Vec<PathBuf>) -> Task<Message> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let mut env = bbl_hostrules::env::Env::current();
        if env.home != home {
            env = bbl_hostrules::env::Env::fixture(&home, env.os);
        }
        let roots: Vec<PathBuf> = roots
            .iter()
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
            .collect();
        if !roots.is_empty() {
            env = env.with_roots(roots);
        } else if bbl_hostrules::resolve::project_roots(&env).is_empty() {
            if let Ok(cwd) = std::env::current_dir() {
                env = env.with_roots(vec![cwd]);
            }
        }
        let inv = linstruct_core::scan(&env);
        let _ = tx.send((Arc::new(inv), started.elapsed().as_millis() as u64));
    });
    Task::perform(
        async move {
            rx.await
                .unwrap_or_else(|_| (Arc::new(Inventory::default()), 0))
        },
        |(inv, ms)| Message::Loaded(inv, ms),
    )
}

/// Titles and details are written with Markdown backticks for the CLI; the app shows them plain.
fn plain(s: &str) -> String {
    s.replace('`', "")
}

fn short_host(name: &str) -> String {
    match name {
        "GitHub Copilot" => "Copilot".into(),
        "JetBrains Junie" => "Junie".into(),
        "JetBrains AI Assistant" => "JetBrains AI".into(),
        n => n.to_string(),
    }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

fn shorten(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if out.len() < s.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_have_the_same_keys() {
        bbl_ui::i18n::same_keys(EN, FR).unwrap();
    }

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(32768), "32\u{202f}768");
        assert_eq!(thousands(1_234_567), "1\u{202f}234\u{202f}567");
    }

    #[test]
    fn every_kind_and_severity_has_its_strings() {
        let s = Strings::load(EN, FR, Lang::En);
        for k in Kind::ALL {
            assert_ne!(
                s.t(&format!("kind.{}", k.slug()), &[]),
                format!("kind.{}", k.slug()),
                "missing kind.{}",
                k.slug()
            );
        }
        for sev in Severity::ALL {
            assert_ne!(
                s.t(&format!("sev.{}", sev.number()), &[]),
                format!("sev.{}", sev.number())
            );
        }
        for w in [
            "launch",
            "on-demand",
            "when-cwd",
            "conditional",
            "manual",
            "never",
        ] {
            assert_ne!(
                s.t(&format!("slot.when.{w}"), &[]),
                format!("slot.when.{w}"),
                "missing slot.when.{w}"
            );
        }
        for o in ["cut", "skip", "undocumented"] {
            assert_ne!(
                s.t(&format!("host.overflow.{o}"), &[]),
                format!("host.overflow.{o}")
            );
        }
    }
}
