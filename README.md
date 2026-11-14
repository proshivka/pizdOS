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
- **v2.0 (Alpine GNU-Free Edition):** лёгкий GNU-free стек (`musl` + `BusyBox`), собственный бинарный формат пакетов `.piz` и пакетный менеджер **`piz-get`** на чистом Rust без единого стороннего крейта.

---

## 🗂️ Структура репозитория

```text
pizdOS/
├── src/
│   ├── bootloader.asm   # 512-байтный MBR-загрузчик (читает ядро через int 13h)
│   └── kernel.asm       # 16-битное ядро, шелл, матрица, CMOS RTC, PC Speaker, BSOD
├── os2/
│   └── overlay/         # Системный оверлей pizdOS 2.0 (MOTD, inittab, сеть udhcpc, /bin команды)
├── scripts/
│   └── build_iso.sh     # Скрипт сборки загрузочного Live ISO (Alpine minirootfs + piz-get)
├── piz-get/
│   ├── Cargo.toml       # [dependencies] абсолютно пустой
│   └── src/
│       ├── main.rs      # CLI, резолвер зависимостей, команды и e2e-тесты
│       ├── pkg.rs       # Бинарный формат пакетов .piz (PIZ1) + защита от path traversal
│       ├── db.rs        # Индекс репозитория (index.db) и база установленного (installed.db)
│       ├── net.rs       # Локальные/file:// репо, HTTP/1.1 на голом TcpStream и HTTPS
│       └── sha256.rs    # Свой SHA-256 с нуля на чистом std
├── Makefile             # Сборка pizdos.img, сборка ISO и запуск в QEMU
├── run.bat              # Запуск pizdOS 1.0 (ASM) через WSL на Windows
├── run2.bat             # Запуск pizdOS 2.0 (Alpine Live ISO) через WSL на Windows
└── LICENSE              # MIT License
```

---

## 🔥 pizdOS 1.0.0 (Bratukha Edition)

### Фичи ядра
- **Настоящий MBR Bootloader (512 байт):** читает ядро напрямую с секторов диска через `int 13h` в `0x1000:0x0000`.
- **Интерактивный шелл (`pizdOS>`):** ввод с клавиатуры через `int 16h`, Backspace, защита от переполнения буфера, эхо-вывод.
- **Поддержка мастей (16 цветов VGA):** меняй цвет текста под настроение командой `color` / `mast`.
- **Настройка разрешения экрана (`res`):** переключение между 80x25, 80x50 и 80x28 строками.
- **Матрица нахуй (`matrix` / `matrica`):** рандомизированный дождь из символов на портах таймера 8254 PIT.
- **Экран критического пиздеца (`panic` / `pizdec`):** собственный аналог BSOD.
- **Прямой опрос CMOS RTC (`time` / `vremya`):** реальное аппаратное время из чипа материнки.
- **PC Speaker Synth (`beep` / `pikni`):** генерация звука через порт `0x61`.
- **Жесткий аппаратный ребут (`reboot` / `vpizdu`):** сброс через контроллер клавиатуры 8042.

---

## 🏔️ pizdOS 2.0 (Alpine GNU-Free Live Edition)

### Особенности редакции
- **100% GNU-free:** Ядро Linux + `musl libc` + `BusyBox`. Никакого glibc и coreutils.
- **Вшитый пакетный менеджер `piz-get`:** скомпилирован статически под `x86_64-unknown-linux-musl`, лежит в `/usr/bin/piz-get`.
- **Автоматическая сеть:** фоновый `udhcpc` поднимает сетевой интерфейс по DHCP.
- **Автологин:** мгновенный вход под пользователем `root` без ввода паролей.
- **Пацанские бинарники в `/bin`:** `nahuy` (clear), `vpizdu` (poweroff), `poyasni` (pizfetch), `ebash` (piz-get install), `snosi` (piz-get remove), `obnovi` (piz-get update).
- **Информатор `pizfetch`:** отображение системных характеристик и ASCII-баннера.

---

## 📦 Пакетный менеджер `piz-get` (Pure Rust, 0 Dependencies)

`piz-get` — штатный пакетный менеджер для **pizdOS 2.0**. Написан полностью на `std`: **никаких `serde`, `reqwest`, `tar`, `sha2` и прочего стороннего хлама**.

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

---

## 🚀 Запуск и сборка

### 1. pizdOS 1.0 (16-bit ASM)
```bash
make all        # Собрать образ build/pizdos.img
make run        # Запустить в QEMU
```
Или на Windows: `run.bat`

### 2. pizdOS 2.0 (Alpine Live ISO)
```bash
make os2-iso    # Собрать образ build/pizdos2.iso
make run-os2    # Запустить pizdOS 2.0 в QEMU
```
Или на Windows: `run2.bat`

---

## 👥 Команда разработчиков (Pizsoft)

- **proshivka** — идеолог, лид, низкоуровневая архитектура
- **Lowyk** — автор пакетного менеджера `piz-get` на чистом Rust
- **KiryxaTipa** — архитектор, разработка системных компонентов
- **Antigravity AI** — ИИ-соавтор и вайбкодер на подхвате

---

## 📜 Лицензия

Распространяется по лицензии **MIT** — потому что GNU пошёл нахуй (делайте чё хотите, только автора не забывайте указывать).
