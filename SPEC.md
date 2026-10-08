# SEN66 → ZiWoAS: Spezifikation

```
SEN66 ──I²C──▶ RP2040 ──USB-CDC, JSON-Zeilen──▶ ZiWoAS (Docker auf dem Homeserver)
```

Die Logik liegt in der Firmware: Sie liest, filtert und verdichtet. ZiWoAS liest die Zeilen direkt vom
seriellen Port, prüft sie, setzt `taken_at` und speichert sie. Keine Einheitenumrechnung und keine Aggregation in ZiWoAS.
Kommentare im Code auf Englisch und sparsam.

## Serielles Zeilenprotokoll (Firmware → ZiWoAS)

Verbindlich für ZiWoAS. USB-CDC, eine kompakte JSON-Zeile pro Nachricht, `\n`-terminiert (ein `\r` davor muss der Leser tolerieren).

```
{"type":"hello","device_id":"…","product":"SEN66","sensor_fw":"<major>.<minor>","firmware":"<git-describe>"}
{"type":"measurement","device_id":"…","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}
{"type":"error","device_id":"…","message":"…"}
```

- `device_id` = SEN66-Seriennummer (`getSerialNumber()`), exakt so, wie die Firmware sie meldet.
- `hello` beim Start, danach alle 10 Minuten und zusätzlich sofort, wenn ein Host den Port öffnet.
- `measurement` alle 60 s. Feldnamen nicht umbenennen, kein Feld weglassen, unbekannt = `null`.
  Schlüsselreihenfolge wie im Beispiel (`type`, `device_id`, dann die Messfelder in Tabellenreihenfolge).
- `error` bei I²C-Fehlern (Text aus `errorToString()`, mit Kontext, welcher Befehl), bei gesetzten Fehlerbits in device_status und bei fehlendem Sensor beim Start. Ist die Seriennummer noch unbekannt, ist `device_id` = `null`.
- JSON-Strings müssen korrekt escaped werden (`"`, `\`, Steuerzeichen).

| Feld | Einheit / Typ |
|---|---|
| pm1_0, pm2_5, pm4_0, pm10 | µg/m³, Float, eine Nachkommastelle, oder null |
| temperature | °C, Float, eine Nachkommastelle, oder null |
| humidity | % r. F., Float, eine Nachkommastelle, oder null |
| voc_index, nox_index | Integer 1–500 oder null |
| co2 | ppm, Integer, oder null |
| device_status | Rohwert von `readDeviceStatus()` (uint32) als Integer |

## Leser in ZiWoAS

Kurzfassung, Details im ZiWoAS-Repo unter `docs/adr/0009-sen66-read-over-usb-serial-inside-the-app.md` (ADR-0009).

- Ein Leseprozess je konfiguriertem Sensor vom Typ `sen66` in `ziwoas.yml`. Konfigurierte `id` = SEN66-Seriennummer,
  `port` = stabiler Pfad `/host-dev/serial/by-id/usb-Adafruit_Feather_RP2040_Adalogger_…-if00`, niemals `/dev/ttyACM0`.
- Gelesen wird mit `stty raw -echo` und `cat`, ohne serielle Bibliothek.
- Zeilen eines anderen Geräts, ungültige Zeilen und `error` werden nur geloggt.
- Jede `measurement` wird eine Zeile in `sensor_readings`; `taken_at` = Empfangszeit (UTC, auf Sekunden).
  Messungen dürfen verloren gehen, es gibt keinen Puffer.
- Port weg (USB gezogen): Neuöffnen mit Backoff (1 s … max. 60 s), ohne Neustart von ZiWoAS.
- Host-Seite (Compose): Host-`/dev` read-only nach `/host-dev` einhängen, `device_cgroup_rules: ['c 166:* rmw']`
  (CDC-ACM-Geräte, auch nach dem Wiedereinstecken), `group_add` mit der Gruppe des Geräteknotens (`dialout`, gid 20).
  Ein `devices:`-Eintrag reicht nicht, er bindet nur den Knoten, der beim Containerstart existiert. Nur
  `/dev/serial/by-id` einzuhängen reicht auch nicht: Die Links sind relativ (`../../ttyACM0`) und zeigen dann ins Leere.

## Firmware (festgelegt)

- arduino-pico-Core `rp2040:rp2040@6.1.1`, FQBN `rp2040:rp2040:adafruit_feather_adalogger`,
  Bibliotheken `Sensirion I2C SEN66@1.3.1` und `Sensirion Core@0.7.3`, gepinnt in `firmware/sketch.yaml`.
- I²C-Adresse `SEN66_I2C_ADDR_6B` (0x6B).
- Start: `deviceReset()`, ca. 1,2 s warten, `getSerialNumber()`, `getVersion()`, `startContinuousMeasurement()`.
  Danach jede Sekunde `getDataReady()` und bei Bereitschaft `readMeasuredValuesAsIntegers()`.
- **Nicht** `readMeasuredValues()` mit Floats verwenden: Die Float-Variante wandelt die „unbekannt“-Werte stumm in Zahlen um.
  Mit der Integer-Variante erkennt die Firmware „unbekannt“ an den Platzhalterwerten: uint16 `0xFFFF` (PM, CO₂), int16 `0x7FFF`
  (Feuchte, Temperatur, VOC, NOx). VOC/NOx-Index 0 (Anlaufphase des Gas-Index-Algorithmus) gilt ebenfalls als unbekannt. Skalierung: PM /10, Feuchte /100, Temperatur /200, VOC /10, NOx /10, CO₂ ×1.
  Vor der Umsetzung gegen den Bibliotheksheader bzw. das Datenblatt prüfen.
- Fenster 60 s (Konstante, zeitbasiert über `millis()`). Median je Größe über die bekannten Einzelwerte. Bei gerader Anzahl
  der Mittelwert der beiden mittleren Werte. Floats auf eine Nachkommastelle runden, Indizes und CO₂ auf ganze Zahlen
  (kaufmännisch, half away from zero). Ist das ganze Fenster unbekannt, gilt `null`.
- `device_status` = `readDeviceStatus()` am Fensterende. Sind Fehlerbits gesetzt (fanError, rhtError, gasError, co22Error,
  hchoError, pmError, co21Error), gibt die Firmware zusätzlich eine `error`-Zeile aus. Die Warnung `fanSpeedWarning` ist kein Fehler.
- Den Sensor zwischen den Fenstern niemals stoppen. Nach I²C-Fehlern mit Backoff neu anlaufen (z. B. 1 s, 2 s, 4 s … bis max. 60 s),
  nicht hängenbleiben. Ein erneutes `deviceReset()` gibt es nur bei einem echten Wiederanlauf nach Fehlern.
- Kein `while (!Serial)`. Ohne Host weitermessen. Ohne Verbindung darf die Ausgabe verloren gehen, sie darf aber nicht blockieren
  (vor dem Schreiben `Serial.availableForWrite()` bzw. `if (Serial)` prüfen).
- Status-LED optional: blinkt beim Messen, leuchtet dauerhaft bei Fehler.
- CO₂-ASC, Höhe/Luftdruck und Temperatur-Offset bleiben auf Werkseinstellung. Konstanten dafür sind vorbereitet, aber inaktiv.
- Firmware-Version über `-DFIRMWARE_VERSION="…"` aus `git describe --always --dirty --tags`, Fallback `"dev"`.
- microSD bleibt ungenutzt.
- Hardwarefreier Kern (reines C++17, keine Arduino-Header) für Fenster/Median/Null-Behandlung/Rundung/JSON-Serialisierung
  in `firmware/src/core/`. Native Tests mit doctest über CMake in `firmware/test/`.

## Repo-Struktur

```
firmware/           Arduino-Sketch (firmware.ino), sketch.yaml, build.sh, src/core/ (hardwarefreier Kern)
firmware/test/      doctest-Tests des Kerns (CMake)
README.md           Verdrahtung, Flashen, Zeilenprotokoll
.github/workflows/  CI: Kern-Tests, arduino-cli compile
```

## Abnahmekriterien

- Nach dem Einstecken steht in ZiWoAS innerhalb von 2 Minuten eine Messung des SEN66.
- Abziehen und Wiedereinstecken: ZiWoAS liest danach wieder, ohne Neustart.
- Ein Neustart von ZiWoAS verliert keine Sensor-Lernphase: Die Firmware misst weiter, VOC- und NOx-Index springen nicht zurück.
- Firmware-Kern-Tests und Firmware-Compile laufen grün in CI.
