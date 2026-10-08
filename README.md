# ziwoas-airquality: SEN66 → ZiWoAS

## Überblick

```
SEN66 ──I²C──▶ RP2040 ──USB-CDC, JSON-Zeilen──▶ ZiWoAS (Docker auf dem Homeserver)
```

- Ein Sensirion SEN66 (PM, Temperatur, Feuchte, VOC, NOx, CO₂) hängt per I²C an einem Adafruit Feather RP2040 Adalogger.
- Die Firmware misst jede Sekunde, bildet über 60 s den Median je Größe und schreibt eine JSON-Zeile über USB.
- Das Board steckt am USB des Homeservers. ZiWoAS liest den seriellen Port selbst, setzt `taken_at` und speichert die Messung.
- Die Logik liegt in der Firmware. Verbindlich ist [SPEC.md](SPEC.md).

## Repo-Struktur

```
firmware/           Arduino-Sketch (firmware.ino), sketch.yaml, build.sh, src/core/ (hardwarefreier Kern)
firmware/test/      doctest-Tests des Kerns (CMake)
.github/workflows/  CI: Kern-Tests, arduino-cli compile
```

## Verdrahtung

### SEN66-Stecker

Am SEN66 sitzt ein 6-poliger Stecker im 1,25-mm-Raster (ACES 51468-0064N-001). Kabelseitig passt
JST GH (`GHR-06V-S`). Pinbelegung laut Datenblatt, Tabelle 16:

| SEN66-Pin | Name | Bedeutung |
|---|---|---|
| 1 | VDD | Versorgung 3,3 V |
| 2 | GND | Masse |
| 3 | SDA | I²C-Daten (5-V-TTL-tolerant) |
| 4 | SCL | I²C-Takt (5-V-TTL-tolerant) |
| 5 | GND | Masse oder NC, intern mit Pin 2 verbunden |
| 6 | VDD | Versorgung oder NC, intern mit Pin 1 verbunden |

Quelle: Sensirion, *SEN6x Datasheet* v0.92 (Dezember 2025), Abschnitt 3 und 4.4:
<https://sensirion.com/resource/datasheet/SEN6x>

### Anschluss an den Feather RP2040 Adalogger

**Standardaufbau (so läuft der Prototyp):** über den
[Adafruit SEN6x Breakout (Produkt 6331)](https://www.adafruit.com/product/6331).

```
Feather RP2040 Adalogger ──STEMMA-QT-Kabel──▶ Adafruit SEN6x Breakout ──JST-GH-6-Kabel──▶ SEN66
     (STEMMA-QT-Buchse)                        (STEMMA QT)   (JST GH)
```

Der Breakout bringt alles mit, was der SEN66 am Bus braucht
([Pinouts im Adafruit-Guide](https://learn.adafruit.com/adafruit-sen6x-breakout/pinouts)):

- **10-kΩ-Pull-ups auf SDA und SCL** (Wert wie im SEN66-Datenblatt empfohlen). Zusätzliche Widerstände sind nicht nötig.
- eigener 3,3-V-Regler für die Sensorversorgung und Pegelwandler (3–5-V-Logik),
- JST-GH-Buchse, in die das 6-polige Kabel des SEN66 direkt passt. Kein Crimpen, keine Belegung prüfen.
- grüne Power-LED. Wer sie im Wohnraum nicht will, trennt die LED-Lötbrücke auf der Rückseite.

SDA/SCL des Feathers liegen auf GPIO2/GPIO3 (I2C1, in Arduino `Wire`); die STEMMA-QT-Buchse führt dieselben Signale.

**Alternative ohne Breakout (Direktverdrahtung):**

```
SEN66 (JST GH 6-pol.)          Feather RP2040 Adalogger
───────────────────────        ─────────────────────────────────────
Pin 1  VDD  ────────────────── 3V
Pin 2  GND  ────────────────── GND
Pin 3  SDA  ──────┬─────────── SDA   = GPIO2
                  └─[10 kΩ]─── 3V
Pin 4  SCL  ──────┬─────────── SCL   = GPIO3
                  └─[10 kΩ]─── 3V
Pin 5  GND  (frei lassen, intern = Pin 2)
Pin 6  VDD  (frei lassen, intern = Pin 1)
```

- **Steckerfalle:** STEMMA QT/Qwiic ist JST **SH**, 4-polig, 1,0 mm. Der SEN66 hat JST **GH**, 6-polig, 1,25 mm.
  Ohne Breakout braucht man ein Adapterkabel oder selbst gecrimpte Leitungen. Belegung vorher mit dem Multimeter prüfen.
- Der Feather hat **keine** eigenen I²C-Pull-ups
  ([Eagle-Schaltplan](https://github.com/adafruit/Adafruit-Feather-RP2040-Adalogger-PCB/blob/main/Adafruit%20Feather%20RP2040%20Adalogger.sch)),
  der SEN66 auch nicht. Die beiden 10-kΩ-Widerstände sind hier also Pflicht.
- Kabel laut Datenblatt: mindestens AWG26, höchstens 50 cm. Sensirion empfiehlt unter 10 cm oder geschirmt.

### Pull-ups und Messwerte

Schwache oder fehlende Pull-ups verfälschen keine Messwerte. Jedes Datenwort des SEN66 ist CRC-gesichert, ein
gestörter Transfer endet als I²C-Fehler, nicht als falscher Wert. Die Firmware meldet ihn als `error`-Zeile
(etwa `getDataReady(): …`) und liest mit Backoff neu. Schlimmstenfalls fehlen einzelne Sekunden im Median oder ein
ganzes Fenster. Häufen sich solche Fehler im ZiWoAS-Log, zuerst Verkabelung und Pull-ups prüfen.

### Elektrische Eckdaten SEN66

| Größe | Wert (Datenblatt, Tabelle 11 und 25) |
|---|---|
| Versorgung | 3,3 V typ. (3,15 … 3,6 V), Welligkeit ≤ 30 mV (SEN66) |
| Strom Messbetrieb | 90 mA typ., 110 mA max. (Mittel über 5 s) |
| Spitzenstrom | 300 mA typ., 350 mA max. (Pulse von 2 ms) |
| Strom Idle | 4,6 mA (erste 10 s), danach 3,3 mA |
| I²C-Adresse | `0x6B` (7 Bit) |
| I²C-Takt | max. 100 kbit/s (Standard Mode), kein Clock Stretching |

Die Energie kommt in beiden Aufbauten aus dem 3,3-V-Regler des Feathers (RT9080-3.3, laut Adafruit 500 mA Spitze)
bei USB-Speisung. Beim Standardaufbau speist er den Breakout, dessen eigener Regler den SEN66 versorgt.
Keine weiteren großen Verbraucher an 3V hängen.

## Firmware

### Voraussetzungen

- [arduino-cli](https://arduino.github.io/arduino-cli/) (macOS: `brew install arduino-cli`)
- `cmake` und ein C++17-Compiler für die Host-Tests (macOS: `brew install cmake`)

Core `rp2040:rp2040@6.1.1` und die Sensirion-Bibliotheken sind im Profil in `firmware/sketch.yaml` gepinnt.
arduino-cli installiert sie beim ersten Build selbst.

### Bauen

```sh
firmware/build.sh
```

`build.sh` kompiliert mit dem Profil aus `sketch.yaml` und setzt `-DFIRMWARE_VERSION` aus
`git describe --always --dirty --tags` (Fallback `dev`).

### Flashen

Port finden: `arduino-cli board list` (macOS `/dev/cu.usbmodemXXXX`, Linux `/dev/ttyACM*` bzw.
`/dev/serial/by-id/usb-Adafruit_Feather_RP2040_Adalogger_…-if00`).

```sh
arduino-cli upload --profile feather -p /dev/cu.usbmodemXXXX firmware
```

arduino-cli setzt das Board per 1200-Baud-Touch in den Bootloader und spielt das UF2 auf. Hat `build.sh` in ein
eigenes Ausgabeverzeichnis gebaut, dieses mit `--input-dir <dir>` angeben.

**Fallback BOOTSEL** (Board hängt oder meldet sich nicht als serielles Gerät):

1. `BOOT` gedrückt halten, `RESET` kurz drücken, `BOOT` loslassen.
2. Das Board erscheint als Laufwerk `RPI-RP2` (USB-ID `2e8a:0003`).
3. Die von `build.sh` erzeugte `.uf2`-Datei auf das Laufwerk ziehen, oder
   `arduino-cli upload --profile feather -p <port> firmware` erneut ausführen. arduino-cli findet das Laufwerk selbst.

### Host-Tests des Kerns

```sh
cmake -S firmware/test -B build/firmware-test && cmake --build build/firmware-test && ctest --test-dir build/firmware-test --output-on-failure
```

### Serielles Zeilenprotokoll (Kurzfassung)

USB-CDC, eine kompakte JSON-Zeile pro Nachricht, `\n`-terminiert. Details stehen in [SPEC.md](SPEC.md).

| `type` | Wann | Inhalt |
|---|---|---|
| `hello` | beim Start, alle 10 min und beim Öffnen des Ports | `device_id`, `product`, `sensor_fw`, `firmware` |
| `measurement` | alle 60 s (Median des Fensters) | `device_id` und alle Messfelder |
| `error` | I²C-Fehler, Fehlerbits in `device_status`, Sensor fehlt | `device_id` (oder `null`), `message` |

```
{"type":"hello","device_id":"0123456789ABCDEF","product":"SEN66","sensor_fw":"4.0","firmware":"v0.1.0"}
{"type":"measurement","device_id":"0123456789ABCDEF","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}
```

### Ausgabe mitlesen

Den Port sollte immer nur ein Prozess lesen. Läuft ZiWoAS mit dem Board, vorher ZiWoAS stoppen.
Die Baudrate spielt bei USB-CDC keine Rolle.

```sh
# macOS
arduino-cli monitor -p /dev/cu.usbmodemXXXX
# oder
screen /dev/cu.usbmodem* 115200        # beenden: Ctrl-A, dann k

# Linux
arduino-cli monitor -p /dev/serial/by-id/usb-Adafruit_Feather_RP2040_Adalogger_*-if00
# oder
stty -F /dev/ttyACM0 raw -echo && cat /dev/ttyACM0
```

Unter macOS `cu.*` nehmen, nicht `tty.*`, denn `tty.*` blockiert beim Öffnen auf DCD.
Unter Linux muss der Benutzer in der Gruppe `dialout` (Debian/Ubuntu) bzw. `uucp` (Arch) sein.

## Anbindung an ZiWoAS

Kurzfassung, Details in [SPEC.md](SPEC.md) und im ZiWoAS-Repo (ADR-0009).

- Board am USB des Homeservers einstecken, stabilen Pfad ermitteln:

  ```sh
  ls -l /dev/serial/by-id/
  # usb-Adafruit_Feather_RP2040_Adalogger_E4629C...-if00 -> ../../ttyACM0
  ```

- In `ziwoas.yml` einen Sensor vom Typ `sen66` eintragen. `id` = Seriennummer aus der `hello`-Zeile,
  `port` = der `by-id`-Pfad unter `/host-dev`, **niemals `/dev/ttyACM0`** (die Nummer hängt von der Reihenfolge der Geräte ab).

  ```yaml
  - id: "19B8E27966467D7A"
    name: "Raumluft"
    type: sen66
    room: "Wohnzimmer"
    port: /host-dev/serial/by-id/usb-Adafruit_Feather_RP2040_Adalogger_…-if00
  ```

- Compose-Dienst von ZiWoAS (am Server getestet):

  ```yaml
  volumes:
    - /dev:/host-dev:ro
  device_cgroup_rules:
    - "c 166:* rmw"
  group_add:
    - "20"   # dialout, Gruppe von /dev/ttyACM*
  ```

  Ein `devices:`-Eintrag reicht nicht, er bindet nur den Knoten, der beim Containerstart existiert. Nur
  `/dev/serial/by-id` einzuhängen reicht auch nicht: Die Links sind relativ (`../../ttyACM0`).

- Abziehen und Wiedereinstecken überbrückt ZiWoAS selbst (Neuöffnen mit Backoff 1 s … 60 s).

## Abnahme / Checks

1. **Einstecken:** Innerhalb von 2 Minuten steht in ZiWoAS eine Messung des SEN66.
   Die erste Messung kommt nach etwa 60 s, bis zum Ende des ersten Fensters.
2. **Abziehen und Wiedereinstecken:** ZiWoAS liest danach wieder, ohne Neustart (das Log zeigt das Neuöffnen mit Backoff).
3. **Neustart von ZiWoAS:** Die Messungen laufen ohne erneute Lernphase weiter, weil die Firmware den Sensor nicht
   stoppt und das Öffnen des Ports das Board nicht zurücksetzt. Die VOC- und NOx-Indizes springen nicht zurück.
4. **CI** ist grün (Kern-Tests, Firmware-Compile).

## Offene Punkte

- **Temperatur-Offset:** Erst gegen ein Referenzthermometer am endgültigen Einbauort messen (mehrere Stunden,
  eingeschwungen). Nur bei nachgewiesener Abweichung per `setTemperatureOffsetParameters()` korrigieren,
  nicht auf Verdacht. Die Konstanten in der Firmware sind vorbereitet, aber inaktiv.
- **CO₂-ASC und Höhe/Luftdruck:** Sie bleiben auf Werkseinstellung (ASC an, keine Höhenkompensation).
  ASC setzt voraus, dass der Raum regelmäßig gut gelüftet wird. Bei deutlich von Meereshöhe abweichendem Standort
  später Höhe oder Luftdruck setzen.
- **microSD-Puffer:** Der Adalogger hat einen microSD-Slot. Ein Puffer für Messungen während Ausfällen von
  ZiWoAS wäre möglich, ist aber bewusst nicht umgesetzt.
