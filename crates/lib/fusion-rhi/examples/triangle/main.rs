#![allow(unused_imports, unused_variables)]

use tracing_subscriber::{filter, prelude::*};
use tracing_appender::non_blocking;
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowBuilder};

use fusion_util as util;
use fusion_rhi as rhi;

pub struct App {
    pub event_loop: Option<EventLoop<()>>,

    dirty_swapchain: bool
}

impl App {
    pub fn new(title: &str) -> Self {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(LogicalSize::new(1360, 768))
            .build(&event_loop)
            .unwrap();

        Self {
            event_loop: Some(event_loop),

            dirty_swapchain: false,
        }
    }

    pub fn run(mut self) -> ! {
        let mut destroying = false;
        self.event_loop.take().unwrap().run(move |event, _, flow| {
            *flow = ControlFlow::Poll;

            match event {
                // Render a frame for this app.
                Event::MainEventsCleared if !destroying => self.render(),
                // Recreate the swapchain on next render.
                Event::WindowEvent { event: WindowEvent::Resized(_), .. } => self.dirty_swapchain = true,
                // Destroy this app.
                Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                    *flow = ControlFlow::Exit;
                    destroying = true;
                    self.destroy();
                },
                Event::LoopDestroyed => {} // Wait gpu idle.
                _ => {}
            }
        });
    }

    pub fn render(&mut self) {
        if self.dirty_swapchain {
            // Recreate swapchain.
        }
        // Draw Frame.
    }

    fn destroy(&self) {

    }
}

fn guarded_main() {
    let app = App::new("Triangle-Example");

}

fn main() {
    let pwd = util::get_cwd().unwrap();
    let filename = "log.txt";
    let logfile = pwd.join(filename);

    let file_appender = tracing_appender::rolling::daily(pwd, filename);

    let (stdout_non_blocking, _stdout_guard) = tracing_appender::non_blocking(std::io::stdout());
    let (file_non_blocking, _file_guard) = tracing_appender::non_blocking(file_appender);

    let stdout_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_file(false)
        .with_line_number(false)
        .with_thread_ids(false)
        .with_target(false);

    let file_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .with_writer(file_non_blocking);

    tracing_subscriber::registry()
        .with(
            stdout_log
        )
        .with(
            file_log
        )
        .init();

    tracing::info!("Logging initialized.");
    tracing::info!("Using logfile: {}", logfile.display());

    guarded_main();
}
