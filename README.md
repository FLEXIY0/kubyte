# kb

Воксельная песочница «мир-как-функция»: `состояние = generate(seed) + diff_log`.
Ноль ассетов, целочисленный детерминизм бит-в-бит, один рендер-бэкенд (wgpu) на все платформы.

**Статус: M0 — скелет.** Окно на десктопе и в WASM, вращающийся куб с процедурной
текстурой камня, ретро-пайплайн (offscreen ⅓ разрешения → nearest-блит) — уже основной
и единственный путь рендера.

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
```

## Крейты

| Крейт | Роль |
|---|---|
| `core` | `no_std`, ноль зависимостей: stateless-хеши, основа детерминизма |
| `materials` | текстура = функция `(дескриптор, сид) → 16×16 RGBA`, integer-only |
| `render` | wgpu: сцена → offscreen → nearest-блит (ретро-пайплайн §6) |
| `platform` | winit-обвязка, точки входа native и wasm |

Полное ТЗ — в `docs/` проекта; принципы: мир — функция, данные вместо кода,
никакого float в генерации, минимум зависимостей.
