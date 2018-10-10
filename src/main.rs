// TODO: add a README

extern crate gdk_pixbuf;
extern crate gtk;
extern crate image;
extern crate nalgebra;

mod render;
mod thread_pool;

macro_rules! autoclone {
  (@param _) => (_);
  (@param $x:ident) => ($x);

  (move || $body:expr) => (move || $body);
  (move |$($p:tt),+| $body:expr) => (move |$(autoclone!(@param $p),)+| $body);

  ($($n:ident),+ => move || $body:expr) => (
    {
      $(let $n = $n.clone();)+
      move || $body
    }
  );

  ($($n:ident),+ => move |$($p:tt),+| $body:expr) => (
    {
      $(let $n = $n.clone();)+
      move |$(autoclone!(@param $p),)+| $body
    }
  );
}

use gdk_pixbuf::{Colorspace, Pixbuf};
use gtk::{
  prelude::*, Builder, Button, FileChooserAction, FileChooserDialog,
  ResponseType, Window,
};
use image::{DynamicImage, GenericImageView};
use render::Renderer;
use std::{cell::RefCell, rc::Rc};

fn main() {
  // TODO: create a GTK Application

  gtk::init().unwrap();

  let main_glade = include_str!("res/main.glade");

  let builder = Builder::new_from_string(main_glade);

  let win =
    Rc::new(RefCell::new(builder.get_object::<Window>("_root").unwrap()));

  let image_preview = Rc::new(RefCell::new(
    builder.get_object::<gtk::Image>("image_preview").unwrap(),
  ));

  let in_img = Rc::new(RefCell::new(None as Option<DynamicImage>));
  let buf = Rc::new(RefCell::new(None as Option<Pixbuf>));

  // TODO: make these configurable
  let renderer = Rc::new(RefCell::new(Renderer::new(64, 64, 10)));

  let open_btn: Button = builder.get_object("open_btn").unwrap();
  let save_btn: Button = builder.get_object("save_btn").unwrap();

  win.borrow_mut().show();

  win.borrow_mut().connect_delete_event(|_, _| {
    gtk::main_quit();
    Inhibit(false)
  });

  open_btn.connect_clicked(autoclone!(win, in_img, renderer => move |_| {
    let dlg = FileChooserDialog::new(
      Some("Open File"),
      Some(&*win.borrow_mut()),
      FileChooserAction::Open,
    );

    dlg.add_buttons(&[
      ("_Cancel", ResponseType::Cancel.into()),
      ("_Open", ResponseType::Accept.into()),
    ]);

    dlg.set_modal(true);

    match ResponseType::from(dlg.run()) {
      ResponseType::Accept => {}
      _ => {
        println!("aborting open");
        dlg.destroy();
        return;
      }
    }

    let files = dlg.get_filenames();

    dlg.destroy();

    if files.is_empty() {
      return;
    } else if files.len() > 1 {
      println!("too many files");

      return;
    }

    gtk::idle_add(autoclone!(image_preview, in_img, buf, renderer => move || {
      let mut img = in_img.borrow_mut();

      println!("loading {:?}", files[0]);

      *img = Some(match image::open(files[0].as_path()) {
        Ok(i) => i,
        Err(e) => {
          println!("  image failed to load: {:?}", e);
          return Continue(false);
        }
      });

      println!("  done");

      let mut buf = buf.borrow_mut();

      let img = img.as_ref().unwrap();

      *buf = Some(Pixbuf::new(
        Colorspace::Rgb,
        true,
        8,
        img.width() as i32,
        img.height() as i32
      ));

      let image_preview = image_preview.borrow_mut();
      let buf = buf.as_ref().unwrap();

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

      Continue(false)
    }));
  }));

  save_btn.connect_clicked(autoclone!(win, renderer => move |_| {
    let img = renderer.borrow_mut().get_output();

    if img.is_some() {
      let dlg = FileChooserDialog::new(
        Some("Save File"),
        Some(&*win.borrow_mut()),
        FileChooserAction::Save,
      );

      dlg.add_buttons(&[
        ("_Cancel", ResponseType::Cancel.into()),
        ("_Save", ResponseType::Accept.into()),
      ]);

      dlg.set_do_overwrite_confirmation(true);
      dlg.set_modal(true);

      match ResponseType::from(dlg.run()) {
        ResponseType::Accept => {}
        _ => {
          println!("aborting save");
          dlg.destroy();
          return;
        }
      }

      let files = dlg.get_filenames();

      dlg.destroy();

      if files.is_empty() {
        return;
      } else if files.len() > 1 {
        println!("too many files");

        return;
      }

      gtk::idle_add(move || {
        println!("saving {:?}", files[0]);

        img.as_ref().unwrap().save(files[0].clone()).unwrap();

        println!(" done");

        Continue(false)
      });
    }
  }));

  gtk::main();
}
