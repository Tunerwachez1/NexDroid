# NexDroid Android Desktop 0.8

Android HOME, системные/пользовательские приложения, закреплённый Google Play (если он установлен), серо-зелёные обои и физическая клавиатура RU/EN/Caps. HardwareIme отправляет события фокуса редактора в Rust NexDroid для автоматического отключения игровых привязок во время набора текста.

Готовый `prebuilt/waydroid-desktop.apk` собран из этих исходников. Его SHA256 лежит рядом. Закрытый ключ подписи не публикуется; APK содержит только публичный сертификат. Весь новый код распространяется под GPL-3.0-or-later, см. корневой LICENSE. Обои созданы для проекта с помощью генерации изображения, не заимствованы у BlueStacks.

Единая установка: [scripts/install-nexdroid.sh](../../scripts/install-nexdroid.sh). Порядок работы и ограничения: [unified-install.md](../../docs/unified-install.md).

## Сборка APK

Нужны JDK17+ и Android SDK platform35/build-tools35.0.0:

```bash
ANDROID_SDK_ROOT=/path/to/sdk JAVA_HOME=/path/to/jdk bash android/desktop/build.sh
```

При первой сборке создаётся личный development-key в игнорируемой `.signing`. Сохраняйте его для собственных обновлений. Самостоятельно подписанный APK не обновит опубликованный APK с другой подписью; установщик не удаляет прежнее приложение ради обхода этой ошибки.

Системные приложения поставляет исходный образ Waydroid. Этот APK не содержит полный Android, Google Play Services, NVIDIA-драйверы, ARM-транслятор или пользовательские данные.
