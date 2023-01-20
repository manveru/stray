use gtk::glib;
use gtk::prelude::*;
use gtk::{IconLookupFlags, IconTheme, Image, Menu, MenuBar, MenuItem, SeparatorMenuItem};
use gtk_layer_shell;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use stray::message::menu::{MenuType, TrayMenu};
use stray::message::tray::{IconPixmap, StatusNotifierItem};
use stray::message::{NotifierItemCommand, NotifierItemMessage};
use stray::StatusNotifierWatcher;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

struct NotifierItem {
    item: StatusNotifierItem,
    menu: Option<TrayMenu>,
}

pub struct StatusNotifierWrapper {
    menu: stray::message::menu::MenuItem,
}

static STATE: Lazy<Mutex<HashMap<String, NotifierItem>>> = Lazy::new(|| Mutex::new(HashMap::new()));

impl StatusNotifierWrapper {
    fn into_menu_item(
        self,
        sender: mpsc::Sender<NotifierItemCommand>,
        notifier_address: String,
        menu_path: String,
    ) -> MenuItem {
        let item: Box<dyn AsRef<MenuItem>> = match self.menu.menu_type {
            MenuType::Separator => Box::new(SeparatorMenuItem::new()),
            MenuType::Standard => Box::new(MenuItem::with_label(self.menu.label.as_str())),
        };

        let item = (*item).as_ref().clone();

        {
            let sender = sender.clone();
            let notifier_address = notifier_address.clone();
            let menu_path = menu_path.clone();

            item.connect_activate(move |_item| {
                sender
                    .try_send(NotifierItemCommand::MenuItemClicked {
                        submenu_id: self.menu.id,
                        menu_path: menu_path.clone(),
                        notifier_address: notifier_address.clone(),
                    })
                    .unwrap();
            });
        };

        let submenu = Menu::new();
        if !self.menu.submenu.is_empty() {
            for submenu_item in self.menu.submenu.iter().cloned() {
                let submenu_item = StatusNotifierWrapper { menu: submenu_item };
                let submenu_item = submenu_item.into_menu_item(
                    sender.clone(),
                    notifier_address.clone(),
                    menu_path.clone(),
                );
                submenu.append(&submenu_item);
            }

            item.set_submenu(Some(&submenu));
        }

        item
    }
}

impl NotifierItem {
    fn get_icon(&self) -> Option<Image> {
        match &self.item.icon_pixmap {
            None => self.get_icon_from_theme(),
            Some(pixmaps) => self.get_icon_from_pixmaps(pixmaps),
        }
    }

    fn get_icon_from_pixmaps(&self, pixmaps: &Vec<IconPixmap>) -> Option<Image> {
        let pixmap = pixmaps
            .iter()
            .find(|pm| pm.height > 20 && pm.height < 32)
            .expect("No icon of suitable size found");

        let pixbuf = gtk::gdk_pixbuf::Pixbuf::new(
            gtk::gdk_pixbuf::Colorspace::Rgb,
            true,
            8,
            pixmap.width,
            pixmap.height,
        )
        .expect("Failed to allocate pixbuf");

        for (y, row) in (0..pixmap.height).zip(
            pixmap
                .pixels
                .chunks_exact((pixmap.width * 4).try_into().expect("invalid pixmap width")),
        ) {
            for (x, pixel) in row.chunks_exact(4).enumerate() {
                let (a, r, g, b) = if let [a, r, g, b] = pixel {
                    (a, r, g, b)
                } else {
                    (&0, &0, &0, &0)
                };
                pixbuf.put_pixel(
                    x.try_into().expect("x coordinate invalid"),
                    y.try_into().expect("y coordinate invalid"),
                    *r,
                    *g,
                    *b,
                    *a,
                );
            }
        }

        Some(Image::from_pixbuf(Some(&pixbuf)))
    }

    fn get_icon_from_theme(&self) -> Option<Image> {
        let theme = gtk::IconTheme::default().unwrap_or(IconTheme::new());
        theme.rescan_if_needed();

        self.item.icon_theme_path.as_ref().map(|path| {
            theme.append_search_path(&path);
        });

        let icon_name = self.item.icon_name.as_ref().unwrap();
        let icon = theme.lookup_icon(icon_name, 24, IconLookupFlags::FORCE_SIZE);

        icon.map(|i| Image::from_pixbuf(i.load_icon().ok().as_ref()))
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let application = gtk::Application::new(
        Some("com.github.gtk-rs.examples.menu_bar_system"),
        Default::default(),
    );

    application.connect_activate(build_ui);

    application.run();
}

fn build_ui(application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);
    window.set_title("System menu bar");
    window.set_border_width(1);
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(500, 500);
    window.connect_screen_changed(set_visual);
    window.connect_draw(draw);

    gtk_layer_shell::init_for_window(&window);
    gtk_layer_shell::auto_exclusive_zone_enable(&window);
    gtk_layer_shell::set_layer(&window, gtk_layer_shell::Layer::Top);
    gtk_layer_shell::set_anchor(&window, gtk_layer_shell::Edge::Left, true);
    gtk_layer_shell::set_anchor(&window, gtk_layer_shell::Edge::Bottom, true);
    gtk_layer_shell::set_anchor(&window, gtk_layer_shell::Edge::Top, false);
    gtk_layer_shell::set_anchor(&window, gtk_layer_shell::Edge::Right, false);
    gtk_layer_shell::set_keyboard_interactivity(&window, false);
    gtk_layer_shell::set_namespace(&window, "gtk-layer-shell");

    let display = gtk::gdk::Display::default()
        .expect("[ERROR] Could not get default display, is your compositor doing okay?");
    let monitor = display
        .monitor(0)
        .expect("[ERROR] Could not find a valid monitor.");
    gtk_layer_shell::set_monitor(&window, &monitor);

    window.set_app_paintable(true);
    window.set_size_request(49, 240);

    let menu_bar = MenuBar::new();
    menu_bar.set_pack_direction(gtk::PackDirection::Btt);
    window.add(&menu_bar);

    let css_provider = gtk::CssProvider::new();
    let style = "menubar { background: rgba(0, 0, 0, 0); }";
    css_provider
        .load_from_data(style.as_bytes())
        .expect("failed loading stylesheet");

    gtk::StyleContext::add_provider_for_screen(
        &gtk::gdk::Screen::default().expect("[ERROR] Couldn't find any valid displays!"),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let (sender, receiver) = mpsc::channel(32);
    let (cmd_tx, cmd_rx) = mpsc::channel(32);

    spawn_local_handler(menu_bar, receiver, cmd_tx);
    start_communication_thread(sender, cmd_rx);
    window.show_all();
}

fn draw(_: &gtk::ApplicationWindow, ctx: &gtk::cairo::Context) -> Inhibit {
    ctx.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    ctx.set_operator(gtk::cairo::Operator::Screen);
    ctx.paint().expect("[ERROR] Failed painting!");
    Inhibit(false)
}

fn set_visual(window: &gtk::ApplicationWindow, screen: Option<&gtk::gdk::Screen>) {
    if let Some(screen) = screen {
        if let Some(ref visual) = screen.rgba_visual() {
            window.set_visual(Some(visual)); // crucial for transparency
        }
    }
}

fn spawn_local_handler(
    v_box: gtk::MenuBar,
    mut receiver: mpsc::Receiver<NotifierItemMessage>,
    cmd_tx: mpsc::Sender<NotifierItemCommand>,
) {
    let main_context = glib::MainContext::default();
    let mut menu_items = std::collections::HashMap::new();

    let future = async move {
        while let Some(item) = receiver.recv().await {
            let mut state = STATE.lock().unwrap();

            match item {
                NotifierItemMessage::Update {
                    address: id,
                    item,
                    menu,
                } => {
                    state.insert(id, NotifierItem { item: *item, menu });
                }
                NotifierItemMessage::Remove { address } => {
                    if let Some(menu_item) = menu_items.get(&address) {
                        v_box.remove(menu_item);
                        menu_items.remove(&address);
                    }

                    state.remove(&address);
                }
            }

            for (address, notifier_item) in state.iter() {
                if let Some(icon) = notifier_item.get_icon() {
                    if !menu_items.contains_key(address) {
                        let menu_item =
                            create_menu_item(&v_box, notifier_item, address, icon, &cmd_tx);
                        menu_items.insert(address.to_string(), menu_item);
                    } else {
                        let menu_item = menu_items.get(address).unwrap();
                        update_menu_item(menu_item, notifier_item, address, icon, &cmd_tx);
                    }
                }
            }

            v_box.show_all();
        }
    };

    main_context.spawn_local(future);
}

fn create_menu_item(
    v_box: &gtk::MenuBar,
    notifier_item: &NotifierItem,
    address: &String,
    icon: gtk::Image,
    cmd_tx: &mpsc::Sender<NotifierItemCommand>,
) -> gtk::MenuItem {
    let menu_item = MenuItem::new();
    let menu_item_box = gtk::Box::default();
    menu_item_box.set_halign(gtk::Align::Center);
    menu_item_box.add(&icon);
    menu_item.add(&menu_item_box);

    if let Some(tray_menu) = &notifier_item.menu {
        let menu = Menu::new();
        tray_menu
            .submenus
            .iter()
            .map(|submenu| StatusNotifierWrapper {
                menu: submenu.to_owned(),
            })
            .map(|item| {
                let menu_path = notifier_item.item.menu.as_ref().unwrap().to_string();
                let address = address.to_string();
                item.into_menu_item(cmd_tx.clone(), address, menu_path)
            })
            .for_each(|item| menu.append(&item));

        if !tray_menu.submenus.is_empty() {
            menu_item.set_submenu(Some(&menu));
        }
    }
    v_box.append(&menu_item);
    menu_item
}

fn update_menu_item(
    menu_item: &gtk::MenuItem,
    notifier_item: &NotifierItem,
    address: &String,
    icon: gtk::Image,
    cmd_tx: &mpsc::Sender<NotifierItemCommand>,
) {
    let children = menu_item.children();
    let menu_item_box = children[0].downcast_ref::<gtk::Box>().unwrap();
    let icons = menu_item_box.children();
    let existing_icon = icons[0].downcast_ref::<gtk::Image>().unwrap();
    existing_icon.set_from_pixbuf(icon.pixbuf().as_ref());

    if let Some(tray_menu) = &notifier_item.menu {
        let sub_menu = menu_item
            .submenu()
            .unwrap()
            .downcast::<gtk::Container>()
            .unwrap();
        sub_menu.foreach(|child| sub_menu.remove(child));
        tray_menu
            .submenus
            .iter()
            .map(|submenu| StatusNotifierWrapper {
                menu: submenu.to_owned(),
            })
            .map(|item| {
                let menu_path = notifier_item.item.menu.as_ref().unwrap().to_string();
                item.into_menu_item(cmd_tx.clone(), address.to_string(), menu_path)
            })
            .for_each(|item| {
                sub_menu
                    .downcast_ref::<gtk::MenuShell>()
                    .unwrap()
                    .append(&item);
            });
    }
}

fn start_communication_thread(
    sender: mpsc::Sender<NotifierItemMessage>,
    cmd_rx: mpsc::Receiver<NotifierItemCommand>,
) {
    thread::spawn(move || {
        let runtime = Runtime::new().expect("Failed to create tokio RT");

        runtime.block_on(async {
            let tray = StatusNotifierWatcher::new(cmd_rx).await.unwrap();
            let mut host = tray.create_notifier_host("MyHost").await.unwrap();

            while let Ok(message) = host.recv().await {
                sender
                    .send(message)
                    .await
                    .expect("failed to send message to UI");
            }

            host.destroy().await.unwrap();
        })
    });
}
