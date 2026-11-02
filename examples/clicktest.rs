//! Style probe: which calls actually recolor a HoldBrowser dark?

use fltk::{
    app, browser::HoldBrowser, button::Button, enums::{Align, FrameType}, frame::Frame,
    group::Flex, prelude::*, window::Window,
};

fn main() {
    let _a = app::App::default();
    app::background(22, 25, 31);
    app::background2(46, 52, 66);
    app::foreground(232, 235, 242);
    app::set_scheme(app::Scheme::Gleam);

    let mut w = Window::default().with_size(420, 300).with_label("style test");
    let mut col = Flex::default_fill().column();
    col.set_pad(10);

    // Variant A: set_color only
    let mut a = HoldBrowser::default();
    a.add("A: set_color only");
    a.set_color(fltk::enums::Color::from_rgb(46, 52, 66));
    col.fixed(&a, 60);

    // Variant B: set_color + set_frame(FlatBox)
    let mut b = HoldBrowser::default();
    b.add("B: color+FlatBox");
    b.set_color(fltk::enums::Color::from_rgb(46, 52, 66));
    b.set_frame(FrameType::FlatBox);
    col.fixed(&b, 60);

    // Variant C: set_color + set_frame + text color via label
    let mut c = HoldBrowser::default();
    c.add("C: color+DownBox+selection");
    c.set_color(fltk::enums::Color::from_rgb(46, 52, 66));
    c.set_frame(FrameType::DownBox);
    c.set_selection_color(fltk::enums::Color::from_rgb(96, 165, 250));
    col.fixed(&c, 60);

    let mut btn = Button::default().with_label("flat box button");
    btn.set_frame(FrameType::FlatBox);
    btn.set_color(fltk::enums::Color::from_rgb(46, 52, 66));
    btn.set_label_color(fltk::enums::Color::from_rgb(232, 235, 242));
    col.fixed(&btn, 30);

    let mut f = Frame::default().with_label("separator below");
    f.set_label_color(fltk::enums::Color::from_rgb(146, 153, 168));
    f.set_label_size(11);
    f.set_align(Align::Left | Align::Inside);
    col.fixed(&f, 18);
    let mut sep = Frame::default();
    sep.set_frame(FrameType::FlatBox);
    sep.set_color(fltk::enums::Color::from_rgb(48, 54, 66));
    col.fixed(&sep, 2);

    col.end();
    w.end();
    w.show();

    while app::wait() {
        if btn.label() == "__quit__" {
            break;
        }
    }
}
