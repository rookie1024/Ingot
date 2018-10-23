use danger::{Danger, DangerWeak};
use filters::{self, Filter};
use gdk_pixbuf::{Colorspace, Pixbuf};
use glib;
use gtk::{
  self, prelude::*, AccelFlags, AccelGroup, Application, ApplicationWindow, Box as GBox, Builder,
  Button, ButtonsType, ComboBoxText, DialogFlags, FileChooserAction, FileChooserDialog, HeaderBar,
  Image as GImage, Label, MessageDialog, MessageType, ProgressBar, ResponseType, Window,
};
use image;
use image::{DynamicImage, GenericImageView};
use num_cpus;
use param_builder;
use render::{DummyRenderProc, RenderCallback, Renderer, TaggedTile};
use std::{
  cell::RefCell,
  cmp,
  collections::{HashMap, VecDeque},
  path::PathBuf,
  rc::Rc,
  sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
  },
};

type AppRenderer = Renderer<AppRenderCallback>;
type RcAppRenderer = Rc<RefCell<AppRenderer>>;

pub type ArcFilter = Arc<Filter + Send + Sync>;

pub fn flt<T>(f: T) -> ArcFilter
where
  T: Filter + Send + Sync + 'static,
{
  Arc::new(f) as ArcFilter
}

pub struct App {
  win: ApplicationWindow,
  header: HeaderBar,
  save_btn: Button,
  image_preview: GImage,
  tool_box: GBox,
  in_img: Rc<RefCell<Option<DynamicImage>>>,
  buf: Arc<Mutex<Option<Danger<Pixbuf>>>>,
  renderer: RcAppRenderer,
  filters: Rc<HashMap<String, ArcFilter>>,
}

impl App {
  pub fn new(gtk_app: &Application, filter_list: Vec<ArcFilter>) -> Self {
    let main_glade = include_str!("res/main.glade");

    let builder = Builder::new_from_string(main_glade);

    let win: ApplicationWindow = builder.get_object("_root").unwrap();

    win.set_application(gtk_app);

    let win_accel_group: AccelGroup = builder.get_object("_root_accel_group").unwrap();

    let header: HeaderBar = builder.get_object("header").unwrap();

    let open_btn: Button = builder.get_object("open_btn").unwrap();
    let save_btn: Button = builder.get_object("save_btn").unwrap();

    let image_preview: GImage = builder.get_object("image_preview").unwrap();

    let filter_select: ComboBoxText = builder.get_object("filter_select").unwrap();

    let tool_box: GBox = builder.get_object("tool_box").unwrap();

    let status_progress: ProgressBar = builder.get_object("status_progress").unwrap();
    let status_text: Label = builder.get_object("status_text").unwrap();

    let buf = Arc::new(Mutex::new(None as Option<Danger<Pixbuf>>));

    let renderer = Self::gen_renderer(&image_preview, &status_progress, &status_text, buf.clone());

    let filters = Rc::new({
      let mut filters = HashMap::new();

      for (i, flt) in vec![
        flt(filters::DummyFilter::new()),
        // flt(filters::PanicFilter::new()),
      ]
      .into_iter()
      .chain(filter_list.into_iter())
      .enumerate()
      {
        let id = i.to_string();

        filter_select.append(id.as_str(), flt.name());
        filters.insert(id, flt);
      }

      filters
    });

    let ret = Self {
      win,
      header,
      save_btn: save_btn.clone(),
      image_preview,
      tool_box,
      in_img: Rc::new(RefCell::new(None)),
      buf,
      renderer,
      filters,
    };

    ret.init(open_btn, save_btn, filter_select, "0");

    ret
  }

  fn gen_renderer(
    image_preview: &GImage,
    status_progress: &ProgressBar,
    status_text: &Label,
    buf: Arc<Mutex<Option<Danger<Pixbuf>>>>,
  ) -> RcAppRenderer {
    let nthreads = num_cpus::get();
    let tile_x: u32 = 64;
    let tile_y: u32 = 64;

    println!(
      "starting renderer\n  {} threads\n  {}x{} tiles",
      nthreads, tile_x, tile_y
    );

    // TODO: make these configurable
    Rc::new(RefCell::new(Renderer::new(
      tile_x,
      tile_y,
      nthreads,
      Arc::new(DummyRenderProc),
      AppRenderCallback::new(
        image_preview.into(),
        status_progress.into(),
        status_text.into(),
        buf,
      ),
    )))
  }

  fn prompt_open_img<W>(parent: Option<&W>) -> Vec<PathBuf>
  where
    W: IsA<Window>,
  {
    let dlg = FileChooserDialog::new(Some("Open Image"), parent, FileChooserAction::Open);

    dlg.add_buttons(&[
      ("_Cancel", ResponseType::Cancel.into()),
      ("_Open", ResponseType::Accept.into()),
    ]);

    dlg.set_modal(true);

    match ResponseType::from(dlg.run()) {
      ResponseType::Accept => {}
      _ => {
        dlg.destroy();
        return Vec::new();
      }
    }

    let files = dlg.get_filenames();

    dlg.destroy();

    files
  }

  fn prompt_save_img<W>(parent: Option<&W>) -> Vec<PathBuf>
  where
    W: IsA<Window>,
  {
    let dlg = FileChooserDialog::new(Some("Save Image"), parent, FileChooserAction::Save);

    dlg.add_buttons(&[
      ("_Cancel", ResponseType::Cancel.into()),
      ("_Save", ResponseType::Accept.into()),
    ]);

    dlg.set_do_overwrite_confirmation(true);
    dlg.set_modal(true);

    match ResponseType::from(dlg.run()) {
      ResponseType::Accept => {}
      _ => {
        dlg.destroy();
        return Vec::new();
      }
    }

    let files = dlg.get_filenames();

    dlg.destroy();

    files
  }

  fn modal_message<W>(parent: Option<&W>, msg: &str, msg_type: MessageType)
  where
    W: IsA<Window>,
  {
    let dlg = MessageDialog::new(parent, DialogFlags::MODAL, msg_type, ButtonsType::Ok, msg);

    dlg.run();

    dlg.destroy();
  }

  fn init(
    &self,
    open_btn: Button,
    save_btn: Button,
    filter_select: ComboBoxText,
    default_filter_id: &str,
  ) {
    //   {
    //     let win = win.borrow_mut();

    //     win.show(); // TODO: figure out why the startup notification has just "."

    //     win.connect_delete_event(|_, _| {
    //       gtk::main_quit();
    //       Inhibit(false)
    //     });

    //     // let (key, mods) = gtk::accelerator_parse("<Control>q");

    //     // TODO: how does this even work? (I mean, it doesn't, but how to I make it work?)
    //     // win.add_accelerator(
    //     //   "unmap",
    //     //   &win_accel_group,
    //     //   key,
    //     //   mods,
    //     //   AccelFlags::VISIBLE,
    //     // );
    //   }

    self.install_open_handler(&open_btn);
    self.install_save_handler(&save_btn);
    self.install_filter_change_handler(&filter_select);

    filter_select.set_active_id(default_filter_id);

    self.win.show_all();
  }

  fn install_open_handler(&self, open_btn: &Button) {
    open_btn.connect_clicked({
      let win = self.win.downgrade();
      let in_img = self.in_img.clone();
      let buf = self.buf.clone();
      let image_preview = self.image_preview.downgrade();
      let renderer = self.renderer.clone();
      let header = self.header.downgrade();
      let save_btn = self.save_btn.downgrade();

      move |_| {
        let win = win.upgrade().unwrap();
        let files = Self::prompt_open_img(Some(&win));

        if files.is_empty() {
          return;
        } else if files.len() > 1 {
          println!("too many files");
          return;
        }

        gtk::idle_add({
          let in_img = in_img.clone();
          let buf = buf.clone();
          let image_preview = image_preview.clone();
          let renderer = renderer.clone();
          let header = header.clone();
          let save_btn = save_btn.clone();

          move || {
            let mut img = in_img.borrow_mut();

            println!("loading {:?}", files[0]);

            *img = Some(match image::open(files[0].as_path()) {
              Ok(i) => i,
              Err(e) => {
                println!("  failed to read image: {:?}", e);

                App::modal_message(
                  Some(&win),
                  &format!("Couldn't open image: {}", e),
                  MessageType::Error,
                );

                return Continue(false);
              }
            });

            println!("  done");

            let mut buf = buf.lock().unwrap();

            let img = img.as_ref().unwrap();

            *buf = Some(
              Pixbuf::new(
                Colorspace::Rgb,
                true,
                8,
                img.width() as i32,
                img.height() as i32,
              )
              .into(),
            );

            let image_preview = image_preview.upgrade().unwrap();
            let buf = &**buf.as_ref().unwrap();

            image_preview.set_from_pixbuf(Some(buf));

            println!("clearing pixbuf...");

            for r in 0..img.height() {
              for c in 0..img.width() {
                buf.put_pixel(c as i32, r as i32, 0, 127, 0, 255);
              }
            }

            println!("  done");

            println!("initializing renderer...");

            renderer.borrow_mut().read_input(img);

            println!("  done");

            let header = header.upgrade().unwrap();
            let save_btn = save_btn.upgrade().unwrap();

            header.set_subtitle(files[0].to_str());

            save_btn.set_sensitive(true);

            Continue(false)
          }
        });
      }
    });
  }

  fn install_save_handler(&self, save_btn: &Button) {
    save_btn.connect_clicked({
      let renderer = self.renderer.clone();
      let win = self.win.downgrade();

      move |_| {
        let img = renderer.borrow_mut().get_output();

        if img.is_some() {
          let win = win.upgrade().unwrap();
          let files = Self::prompt_save_img(Some(&win));

          if files.is_empty() {
            return;
          } else if files.len() > 1 {
            println!("too many files");
            return;
          }

          gtk::idle_add({
            //
            move || {
              println!("saving {:?}", files[0]);

              match img.as_ref().unwrap().save(files[0].clone()) {
                Ok(_) => (),
                Err(e) => {
                  println!("  failed to write image: {:?}", e);

                  App::modal_message(
                    Some(&win),
                    &format!("Couldn't save image: {}", e),
                    MessageType::Error,
                  );

                  // TODO: delete any accidentally created files
                  return Continue(false);
                }
              }

              println!("  done");

              Continue(false)
            }
          });
        }
      }
    });
  }

  fn install_filter_change_handler(&self, filter_select: &ComboBoxText) {
    filter_select.connect_changed({
      let renderer = self.renderer.clone();
      let filters = self.filters.clone();
      let tool_box = self.tool_box.downgrade();

      move |el| {
        let id = match el.get_active_id() {
          Some(i) => i,
          None => return,
        };

        let flt = &filters[&id];

        renderer.borrow_mut().set_proc(flt.proc());

        let tool_box = tool_box.upgrade().unwrap();

        param_builder::build(&tool_box, flt.params(), &renderer);
      }
    });
  }
}

struct AppRenderCallbackTag {
  queued: AtomicUsize,
}

impl Default for AppRenderCallbackTag {
  fn default() -> Self {
    Self {
      queued: AtomicUsize::new(0),
    }
  }
}

type AppTaggedTile = TaggedTile<AppRenderCallbackTag>;

#[derive(Clone)]
struct AppRenderCallback {
  done: Arc<AtomicUsize>,
  total: Arc<AtomicUsize>,
  running: Arc<AtomicBool>,
  image_preview: DangerWeak<GImage>,
  status_progress: DangerWeak<ProgressBar>,
  status_text: DangerWeak<Label>,
  buf: Arc<Mutex<Option<Danger<Pixbuf>>>>,
  q: Arc<Mutex<VecDeque<Arc<AppTaggedTile>>>>,
}

impl AppRenderCallback {
  fn new(
    image_preview: DangerWeak<GImage>,
    status_progress: DangerWeak<ProgressBar>,
    status_text: DangerWeak<Label>,
    buf: Arc<Mutex<Option<Danger<Pixbuf>>>>,
  ) -> Self {
    Self {
      done: Arc::new(AtomicUsize::new(0)),
      total: Arc::new(AtomicUsize::new(0)),
      running: Arc::new(AtomicBool::new(false)),
      image_preview,
      status_progress,
      status_text,
      buf,
      q: Arc::new(Mutex::new(VecDeque::new())),
    }
  }

  fn dispatch_worker(&self) {
    glib::idle_add({
      let buf = self.buf.clone();
      let image_preview = self.image_preview.clone();
      let status_progress = self.status_progress.clone();
      let status_text = self.status_text.clone();
      let q = self.q.clone();
      let done = self.done.clone();
      let total = self.total.clone();
      let running = self.running.clone();

      move || {
        let mut did_work = false;

        let out_buf = buf.lock().unwrap();

        let out_buf = match &*out_buf {
          Some(b) => &**b,
          None => return Continue(false),
        };

        let image_preview = image_preview.upgrade().unwrap();
        let status_progress = status_progress.upgrade().unwrap();
        let status_text = status_text.upgrade().unwrap();

        let mut q = q.lock().unwrap();

        let len = q.len();

        for tile in q.drain(0..cmp::min(500, len)) {
          did_work = true;

          if tile.tag().queued.fetch_sub(1, Ordering::SeqCst) == 1 {
            let tile = tile.tile();

            let tile_buf = tile.out_buf();

            for r in 0..tile.h() {
              let r_stride = r * tile.w();

              for c in 0..tile.w() {
                let px = tile_buf[(r_stride + c) as usize];

                out_buf.put_pixel(
                  (tile.x() + c) as i32,
                  (tile.y() + r) as i32,
                  (px[0] * 255.0).round() as u8,
                  (px[1] * 255.0).round() as u8,
                  (px[2] * 255.0).round() as u8,
                  (px[3] * 255.0).round() as u8,
                );
              }
            }
          }
        }

        image_preview.set_from_pixbuf(Some(out_buf));

        let done = done.load(Ordering::SeqCst);
        let total = total.load(Ordering::SeqCst);

        status_progress.set_fraction(done as f64 / total as f64);
        status_text.set_text(&format!("{} / {}", done, total));

        if !did_work {
          running.store(false, Ordering::SeqCst);
        }

        Continue(did_work)
      }
    });
  }
}

impl RenderCallback for AppRenderCallback {
  type Tag = AppRenderCallbackTag;

  // TODO: disable the save button during rendering

  fn before_begin(&self, ntiles: usize) {
    self.total.store(ntiles, Ordering::SeqCst);
    self.done.store(0, Ordering::SeqCst);
  }

  fn abort(&self) {
    if self.running.load(Ordering::SeqCst) {
      let mut q = self.q.lock().unwrap();

      for tile in q.drain(..) {
        tile.tag().queued.store(0, Ordering::SeqCst);
      }
    }
  }

  fn handle_tile(&self, tile: Arc<AppTaggedTile>) {
    // TODO: determine if Danger<Pixbuf> is safe enough to blit to from another thread

    tile.tag().queued.fetch_add(1, Ordering::SeqCst);
    self.done.fetch_add(1, Ordering::SeqCst);
    self.q.lock().unwrap().push_back(tile);

    if !self.running.swap(true, Ordering::SeqCst) {
      self.dispatch_worker();
    }
  }
}
