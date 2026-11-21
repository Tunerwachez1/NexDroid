# Дополнительно / Advanced

Откройте шестерёнку внутри Android-окна → **Дополнительно**. Android должен быть запущен: вкладка читает фактические значения и не сохраняет неизвестное состояние.

- **ABI**: список архитектур, объявленных работающим образом Android, включая установленные ARM bridge ABI. Это информационное поле, не переключатель архитектуры.
- **Сетевой ADB**: включение/выключение ADB в контейнере. При включении используется TCP 5555. Вкладка показывает фактический адрес и копирует команду `adb connect <container-ip>:5555`. Это адрес Waydroid, а не `127.0.0.1`. В Linux нужен клиент adb; при первом реальном соединении подтвердите ключ компьютера в Android. Образ должен использовать `ro.adb.secure=1`; авторизация ADB не отключается.
- **Показывать касания** и **Координаты указателя**: независимые Android settings, применяются после сохранения. Отладочные overlays могут перекрывать игровые элементы.

После «Сохранить изменения» helper применяет настройки, проверяет результат и сохраняет `/var/lib/nexdroid/advanced.json` (root-owned, 0600). При подключении NexDroid после загрузки Android сохранённые значения восстанавливаются. Пока файл не создан явным сохранением, запуск не меняет эти настройки. Кнопка обновления перечитывает фактическое состояние Android.

ADB проверяется по реальному TCP LISTEN в namespace Android, а не только по `init.svc.adbd`. Для остановки helper ждёт точного `stopped`; если adbd завис в `stopping`, завершается только фиксированный Android-процесс adbd. Для включения используется последовательность полного stop и start с подтверждением сетевого порта. Контейнер, игра и графический драйвер не перезапускаются.

Привилегированный API принимает только три boolean. Произвольные shell-команды, имена свойств, процессы, адреса и порты пользователь передать не может. Используется существующее запоминаемое разрешение настроек NexDroid. При ошибке helper пытается вернуть предыдущие значения и не записывает новый JSON.

## English

Gear → **Advanced** shows the running image's ABI list and separate switches for network ADB, tap indicators and pointer coordinates. Save applies and verifies the values without restarting the Android container or game. Network ADB uses the container address, normally port 5555; install the Linux adb client and approve the computer key in Android. ADB authentication is retained. Saved debugging settings are restored when NexDroid connects after Android boots.
