# kb

Воксельная песочница «мир-как-функция»: `состояние = generate(seed) + diff_log`.
Ноль ассетов, целочисленный детерминизм бит-в-бит, один рендер-бэкенд (wgpu) на все платформы.

**Статус: M4 (первая половина) — жизнь.** Бесконечный мир с деревьями,
выживание (физика, ломание/установка, дифф-лог, сейв), цветное освещение
skylight+blocklight (холодная ночь / тёплый янтарь ламп), день/ночь,
дизеринг Байера. Ретро-пайплайн — единственный путь рендера с M0.

**Управление:** WASD — ходьба, Space — прыжок, мышь — взгляд (клик —
захват, Esc — отпустить), ЛКМ/ПКМ — сломать/поставить, 1–6 — блок в руке
(камень/земля/трава/дерево/листва/фонарь), F — полёт.

## Сборка

```sh
# Десктоп
cargo run --release

# Браузер (нужны: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli)
cargo build --release --target wasm32-unknown-unknown -p kb
wasm-bindgen --target web --no-typescript \
    --out-dir web/pkg target/wasm32-unknown-unknown/release/kb.wasm
# затем раздать web/ любым статик-сервером, например:
python3 -m http.server -d web

# Тесты детерминизма (§16) — должны проходить и на native, и на wasm32
cargo test

# Замер против бюджетов §9 (время генерации чанка, память)
cargo run --release -p kb-worldgen --example bench
```

### Готовые сборки (CI)

Каждый пуш собирает артефакты в GitHub Actions (вкладка Actions → последний
прогон → Artifacts): `kb-linux-x86_64`, `kb-windows-x86_64` — бинарники,
`kb-web` — веб-бандл (распаковать, раздать статиком: `python3 -m http.server -d kb-web`).
С ветки `main` веб-версия дополнительно деплоится на GitHub Pages
(нужно один раз включить в Settings → Pages → Source: GitHub Actions).

## Крейты

| Крейт | Роль |
|---|---|
| `core` | `no_std`, ноль зависимостей: stateless-хеши, блоки, палитро-чанки |
| `worldgen` | детерминированный генератор: integer fBm, версионируется (§4) |
| `materials` | текстура = функция `(дескриптор, сид) → 16×16 RGBA`, integer-only |
| `render` | wgpu: сцена → offscreen → nearest-блит (ретро-пайплайн §6) |
| `platform` | winit-обвязка, точки входа native и wasm |

Полное ТЗ — в `docs/` проекта; принципы: мир — функция, данные вместо кода,
никакого float в генерации, минимум зависимостей.
