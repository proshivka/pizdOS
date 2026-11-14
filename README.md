# 💀 pizdOS (PDOS)

<p align="center">
  <pre>
  ______ _____ ______ _____   ____   _____ 
 |  __ \_   _|___  /|  __ \ / __ \ / ____|
 | |__) || |    / / | |  | | |  | | (___  
 |  ___/ | |   / /  | |  | | |  | |\___ \ 
 | |    _| |_ / /__ | |__| | |__| |____) |
 |_|   |_____/_____|_____/ \____/|_____/  
  </pre>
  <i>«Какая-то ебануто хуёвая, но охуенно быстрая операционная система»</i><br>
  <b>0 чужих зависимостей • 0 багов (только фичи) • 0% GNU</b>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Arch-x86_16--bit_Real_Mode_%7C_x86__64-red.svg" alt="Arch">
  <img src="https://img.shields.io/badge/Kernel-NASM_x86_Assembly-blue.svg" alt="Kernel">
  <img src="https://img.shields.io/badge/piz--get-Pure_Rust_(0_deps)-orange.svg" alt="piz-get">
  <img src="https://img.shields.io/badge/Bugs-0_(Only_Features)-brightgreen.svg" alt="Bugs">
  <img src="https://img.shields.io/badge/GNU%2FGPL-Nahuy-black.svg" alt="No GNU">
  <img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="MIT">
</p>

---

## ⚡ О проекте

**pizdOS** — это бескомпромиссная операционная система и экосистема, созданная для тех, кому осточертели неповоротливые гиганты вроде Windows и раздутые дистрибутивы с тоннами чужих зависимостей.

Наша философия простая:
- **Чужие зависимости = чужие баги.** У нас багов нет — у нас только фичи.
- **v1.0.0 (Bratukha Edition):** чистый 16-битный ассемблер x86, прямое общение с BIOS и железом, старт с загрузочного сектора за `0.0001` секунды.
- **v2.0 (В разработке):** лёгкий GNU-free стек (`musl` + `BusyBox`), собственный бинарный формат пакетов `.piz` и пакетный менеджер **`piz-get`** на чистом Rust без единого стороннего крейта.

---

## 🗂️ Структура репозитория

```text
pizdOS/
├── src/
│   ├── bootloader.asm   # 512-байтный MBR-загрузчик (читает ядро через int 13h)
│   └── kernel.asm       # 16-битное ядро, шелл, матрица, CMOS RTC, PC Speaker, BSOD
├── piz-get/
│   ├── Cargo.toml       # [dependencies] абсолютно пустой
│   └── src/
│       ├── main.rs      # CLI, резолвер зависимостей, команды и e2e-тесты
│       ├── pkg.rs       # Бинарный формат пакетов .piz (PIZ1) + защита от path traversal
│       ├── db.rs        # Индекс репозитория (index.db) и база установленного (installed.db)
│       ├── net.rs       # Локальные/file:// репо, HTTP/1.1 на голом TcpStream и HTTPS
│       └── sha256.rs    # Свой SHA-256 с нуля на чистом std
├── Makefile             # Сборка образа pizdos.img и запуск в QEMU
├── run.bat              # Быстрый запуск через WSL на Windows
└── LICENSE              # MIT License
```

---

## 🔥 pizdOS 1.0.0 (Bratukha Edition)

### Фичи ядра
- **Настоящий MBR Bootloader (512 байт):** читает ядро напрямую с секторов диска через `int 13h` в `0x1000:0x0000`.
- **Интерактивный шелл (`pizdOS>`):** ввод с клавиатуры через `int 16h`, Backspace, защита от переполнения буфера, эхо-вывод.
- **Поддержка мастей (16 цветов VGA):** меняй цвет текста под настроение командой `color` / `mast`.
- **Матрица нахуй (`matrix` / `matrica`):** рандомизированный дождь из символов на портах таймера 8254 PIT.
- **Экран критического пиздеца (`panic` / `pizdec`):** собственный аналог BSOD.
- **Прямой опрос CMOS RTC (`time` / `vremya`):** реальное аппаратное время из чипа материнки.
- **PC Speaker Synth (`beep` / `pikni`):** генерация звука через порт `0x61`.
- **Жесткий аппаратный ребут (`reboot` / `vpizdu`):** сброс через контроллер клавиатуры 8042.

### Команды шелла

| Команда (Eng) | По-пацански | Описание |
| :--- | :--- | :--- |
| `help` | `huelp` | Вывести список всех доступных команд |
| `clear` | `nahuy` | Нахуй очистить весь экран |
| `echo <текст>` | `pizdi <текст>` | Вывести введённый базар в консоль |
| `info` | `poyasni` | Пояснить за систему и проверить размер RAM |
| `time` | `vremya` | Показать точное аппаратное время из CMOS |
| `beep` | `pikni` | Издать громкий писк через спикер |
| `color <0-F>` | `mast <0-F>` | Сменить цвет шрифта (`1`=Синий, `2`=Зелёный, `4`=Красный, `E`=Жёлтый) |
| `matrix` | `matrica` | Запустить бесконечную матрицу (выход — любая клавиша) |
| `panic` | `pizdec` | Вызвать Kernel Panic / экран критического пиздеца |
| `reboot` | `vpizdu` | Перезагрузить ПК через контроллер 8042 |

---

## 📦 Пакетный менеджер `piz-get` (Pure Rust, 0 Dependencies)

`piz-get` — штатный пакетный менеджер для **pizdOS 2.0**. Написан полностью на `std`: **никаких `serde`, `reqwest`, `tar`, `sha2` и прочего стороннего хлама**.

### Что умеет из коробки
- **Свой бинарный формат `.piz` (`PIZ1`):** хранит манифест (`piz.manifest`), права доступа Unix и файлы с проверкой путей (блокирует `..` и абсолютные пути).
- **Свой SHA-256:** каждый скачанный пакет проверяется по контрольной сумме перед распаковкой.
- **Граф зависимостей:** автоматический резолв зависимостей при установке, детекция циклов и защита от сноса нужных другим пакетам библиотек (если не указан `--force`).
- **Транспорт без крейтов:** локальные директории и `file://`, чистый **HTTP/1.1** поверх `std::net::TcpStream` (включая `chunked` и редиректы) + фолбэк на `wget`/`curl` для `https://`.
- **Чистое удаление:** `installed.db` помнит каждый файл пакета и подчищает пустые директории при удалении.

### Команды `piz-get`

| Команда (Eng) | По-пацански | Что делает |
| :--- | :--- | :--- |
| `update` | `obnovi` | Обновить `index.db` из репозитория (`--repo` / `$PIZ_REPO`) |
| `install <pkg...>` | `stav` / `ebash` | Установить пакеты вместе с зависимостями |
| `remove <pkg...>` | `snosi` / `nahuy` | Удалить пакеты и их файлы (`--force` игнорирует зависимости) |
| `upgrade` | `apgreyd` | Обновить все установленные пакеты до версий из `index.db` |
| `list` | `spisok` | Показать установленные пакеты (`-a` — доступные в индексе) |
| `search <query>` | `ishi` | Найти пакеты по названию или описанию |
| `info <pkg>` | `poyasni` | Показать статус, зависимости, размер и SHA-256 пакета |
| `pack <dir> [out]` | `pakuy` | Упаковать директорию с `piz.manifest` в `.piz` архив |
| `index <dir>` | — | Сгенерировать `index.db` для папки с `.piz` пакетами |

### Пример: собрать свой `.piz` пакет и поднять локальный репо

```bash
# 1. Собрать piz-get в release (оптимизирован по размеру)
cd piz-get
cargo build --release

# 2. Создать структуру пакета
mkdir -p mypkg/usr/bin
cat << 'EOF' > mypkg/piz.manifest
name=pizfetch
version=0.1.0
arch=x86_64
desc=Show pizdOS system info
depends=
EOF
echo -e '#!/bin/sh\necho "pizdOS 2.0!"' > mypkg/usr/bin/pizfetch
chmod +x mypkg/usr/bin/pizfetch

# 3. Запаковать и проиндексировать репозиторий
mkdir -p repo rootfs
./target/release/piz-get pack mypkg repo/pizfetch-0.1.0-x86_64.piz
./target/release/piz-get index repo

# 4. Обновить индекс и поставить пакет в rootfs
./target/release/piz-get --root rootfs --repo repo update
./target/release/piz-get --root rootfs --repo repo stav pizfetch
```

---

## 🚀 Запуск и сборка

### 1. Сборка и запуск ОС (pizdOS 1.0.0)
Требуются `nasm`, `qemu-system-x86_64`, `make`.

**На Windows (через WSL):**
```cmd
run.bat
```

**В терминале / Linux / WSL:**
```bash
# Собрать образ build/pizdos.img
make all

# Запустить в QEMU
make run

# Запустить прямо в терминале (curses)
make run-curses
```

### 2. Сборка и тесты `piz-get`
Требуется только Rust (`rustc` / `cargo`).

```bash
cd piz-get
cargo test
cargo build --release
```

---

## 🗺️ Roadmap: pizdOS 2.0 (PDOS 2)

- [x] 📦 Кастомный пакетный менеджер **`piz-get`** (pure Rust, 0 dependencies, формат `.piz`, SHA-256, резолв зависимостей).
- [ ] 🏔️ Базирование на **Alpine Linux** (чистый **GNU-free** стек: `musl` + `BusyBox`).
- [ ] ⚡ Легковесный Live-образ / дистрибутив с предустановленным `piz-get`.
- [ ] 🖼️ Интеграция **Pizsoft UI**.

---

## 📜 Лицензия

Распространяется по лицензии **MIT** — потому что GNU пошёл нахуй (делайте чё хотите, только автора не забывайте указывать).
