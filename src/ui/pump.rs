use gpui::*;

pub fn pump<T: 'static, M: Send + 'static>(
    cx: &mut Context<T>,
    work: impl FnOnce(smol::channel::Sender<M>) + Send + 'static,
    mut apply: impl FnMut(&mut T, M, &mut Context<T>) + 'static,
) {
    let (tx, rx) = smol::channel::unbounded::<M>();
    std::thread::spawn(move || work(tx));
    cx.spawn(async move |this, cx| {
        while let Ok(msg) = rx.recv().await {
            if this.update(cx, |page, cx| apply(page, msg, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}