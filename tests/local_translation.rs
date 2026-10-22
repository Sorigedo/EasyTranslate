use easytranslate::local_ai::{LocalTranslator, RouteLoader, TranslationRoute};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

struct StubRoute(String);

impl TranslationRoute for StubRoute {
    fn translate_long(&self, text: &str) -> String {
        format!("{}:{text}", self.0)
    }
}

struct StubLoader {
    loads: AtomicUsize,
    routes: Mutex<Vec<(String, String)>>,
}

impl RouteLoader for StubLoader {
    fn load(&self, source: &str, target: &str) -> Result<Box<dyn TranslationRoute>, String> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        self.routes
            .lock()
            .unwrap()
            .push((source.into(), target.into()));
        Ok(Box::new(StubRoute(format!("{source}>{target}"))))
    }
}

#[test]
fn local_translator_reuses_a_loaded_direction_but_keeps_reverse_routes_separate() {
    let loader = Arc::new(StubLoader {
        loads: AtomicUsize::new(0),
        routes: Mutex::new(Vec::new()),
    });
    let translator = LocalTranslator::with_loader(loader.clone());

    assert_eq!(
        translator.translate("你好", "zh-Hans", "en").unwrap(),
        "zh-Hans>en:你好"
    );
    assert_eq!(
        translator.translate("再见", "zh-Hans", "en").unwrap(),
        "zh-Hans>en:再见"
    );
    assert_eq!(
        translator.translate("Hello", "en", "zh-Hans").unwrap(),
        "en>zh-Hans:Hello"
    );

    assert_eq!(loader.loads.load(Ordering::SeqCst), 2);
    assert_eq!(
        *loader.routes.lock().unwrap(),
        vec![
            ("zh-Hans".into(), "en".into()),
            ("en".into(), "zh-Hans".into())
        ]
    );
}

#[test]
fn local_translator_returns_model_download_errors_without_a_translation() {
    struct FailingLoader;
    impl RouteLoader for FailingLoader {
        fn load(&self, _source: &str, _target: &str) -> Result<Box<dyn TranslationRoute>, String> {
            Err("model CDN unavailable".into())
        }
    }

    let translator = LocalTranslator::with_loader(Arc::new(FailingLoader));
    assert_eq!(
        translator.translate("你好", "zh-Hans", "en"),
        Err("model CDN unavailable".into())
    );
}

#[test]
fn cancelled_translation_does_not_wait_for_model_preparation_to_finish() {
    struct SlowLoader;
    impl RouteLoader for SlowLoader {
        fn load(&self, _source: &str, _target: &str) -> Result<Box<dyn TranslationRoute>, String> {
            std::thread::sleep(Duration::from_millis(250));
            Ok(Box::new(StubRoute("slow".into())))
        }
    }

    let translator = LocalTranslator::with_loader(Arc::new(SlowLoader));
    let cancellation = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let cancel_later = cancellation.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        cancel_later.store(2, Ordering::SeqCst);
    });
    let started = Instant::now();
    assert_eq!(
        translator.translate_cancellable("hello", "en", "fr", Some((cancellation, 1))),
        Err("翻译已取消".into())
    );
    assert!(started.elapsed() < Duration::from_millis(150));
}

#[test]
fn identical_source_and_target_languages_do_not_download_a_route() {
    struct MustNotLoad;
    impl RouteLoader for MustNotLoad {
        fn load(&self, _source: &str, _target: &str) -> Result<Box<dyn TranslationRoute>, String> {
            panic!("same-language requests should bypass model loading")
        }
    }

    let translator = LocalTranslator::with_loader(Arc::new(MustNotLoad));
    assert_eq!(
        translator.translate("原样保留", "zh-Hans", "zh-Hans"),
        Ok("原样保留".into())
    );
}

#[test]
fn preload_loads_the_default_route_without_translating_user_text() {
    let loader = Arc::new(StubLoader {
        loads: AtomicUsize::new(0),
        routes: Mutex::new(Vec::new()),
    });
    let translator = LocalTranslator::with_loader(loader.clone());

    translator.preload("zh-Hans", "en").unwrap();

    assert_eq!(loader.loads.load(Ordering::SeqCst), 1);
    assert_eq!(
        *loader.routes.lock().unwrap(),
        vec![("zh-Hans".into(), "en".into())]
    );
}
