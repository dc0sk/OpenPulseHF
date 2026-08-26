# German content for the overview deck. Structure mirrors slides.py exactly; gen.py, diagrams.py and
# the render check are shared, so a layout fix applies to both languages.
#
# TERMINOLOGY, decided once here so it stays consistent:
#   HF (3–30 MHz)     -> Kurzwelle / KW.  German "HF" means Hochfrequenz generally, not the band.
#   callsign          -> Rufzeichen
#   transmission      -> Aussendung;  to key the transmitter -> tasten
#   mode (waveform)   -> Betriebsart
#   frame             -> Frame (established in German digital-mode usage)
#   decode            -> dekodieren / Dekodierung
# Protocol and product names stay untranslated: Winlink, Pat, ARDOP, KISS, AX.25, BPSK, OFDM, ARQ.
#
# LINE LENGTH: German runs 15–30 % longer than English, and the body frame clips silently. Keep body
# lines under ~78 characters and CHECK WITH A RENDER — check_render.py is the only thing that sees it.

URL = "https://dc0sk.github.io/OpenPulseHF"
FOOTER = "OpenPulseHF — ein Open-Source-Projekt von Simon Keimer (DC0SK)"

USE_CASES = [
 ("Anwendungsfall 1 — Nachrichten ohne Netz", [
   "Ein Segler mitten im Atlantik. Ein Einsatzteam außerhalb jeder Mobilfunkzelle.",
   "Eine Region, in der schlicht kein Netz existiert.",
   "",
   "Kurzwelle braucht zwischen den Endpunkten keine Infrastruktur — nur Ausbreitung.",
   "",
   "Was OpenPulseHF dafür mitbringt:",
   "• Vollständiger B2F-/Winlink-Sessiontreiber und ein direktes CMS-Gateway",
   "• ARDOP-TCP-Schnittstelle mit dem Befehlssatz, den Pat bereits erwartet",
   "• Kompression, Segmentierung und Reassemblierung über 255 Byte hinaus",
   "",
   "Ehrlicher Rahmen: Der Winlink-Stack ist implementiert und End-to-End getestet,",
   "aber im Loopback. Typ D (gzip) wird unterstützt, Typ C (LZHUF) abgelehnt."]),

 ("Anwendungsfall 2 — Notfunk", [
   "Wenn die Infrastruktur selbst der Schaden ist: Sturm, Hochwasser, Erdbeben.",
   "",
   "Was in dieser Stunde zählt, ist nicht Durchsatz. Es ist: Kommt eine Nachricht",
   "überhaupt durch, und kann der Empfänger erkennen, wer sie gesendet hat?",
   "",
   "Was OpenPulseHF dafür mitbringt:",
   "• Eine Wellenform unterhalb des Rauschflurs (MFSK16) für schwächste Verbindungen",
   "• Signierte Identität im Funkkanal — der Empfänger prüft, wer gesendet hat",
   "• Automatische Ratenanpassung, die auf Dekodierungen reagiert, nicht auf Modelle",
   "",
   "Ehrlicher Rahmen: Das ist Technik, kein Notfunkkonzept. Betriebsabläufe, Alarmierung",
   "und Verantwortlichkeiten bleiben Sache der jeweiligen Organisation."]),

 ("Anwendungsfall 3 — Dateien zwischen Stationen", [
   "Ein Foto, ein Lagebericht, eine Konfigurationsdatei — ohne Internet dazwischen.",
   "",
   "Was OpenPulseHF dafür mitbringt:",
   "• Eigenes P2P-Dateitransferprotokoll (OPFX) mit Angebot, Manifest und Richtlinie",
   "• Blockweise Übertragung mit Fragment-Bitmaps und Wiederaufnahme nach Abbruch",
   "• Signiertes Manifest: Inhalt und Absender werden vor der Annahme geprüft",
   "• Sendepausen sind zeitlich begrenzt, damit keine Dauer-Tastung entsteht",
   "",
   "Ehrlicher Rahmen: Über zwei echte Daemons im Loopback getestet, auf Luftschnittstelle",
   "noch nicht. Kurzwelle ist langsam — das hier ist für Kilobyte, nicht Megabyte."]),

 ("Anwendungsfall 4 — Gegenstation finden", [
   "Auf Kurzwelle weiß man selten, wer gerade erreichbar ist. Ausbreitung entscheidet.",
   "",
   "Was OpenPulseHF dafür mitbringt:",
   "• JS8-kompatible Bake mit eigener Kennung, damit andere Stationen uns erkennen",
   "• Stationstabelle aus dem, was tatsächlich gehört wurde — kein zentraler Server",
   "• Rendezvous: zwei Stationen einigen sich auf eine Frequenz und wechseln gemeinsam",
   "• Danach übernimmt der signierte HPX-Handshake die eigentliche Verbindung",
   "",
   "Ehrlicher Rahmen: Ab Werk abgeschaltet. Es braucht ein Rufzeichen, eine Uhr auf",
   "±2 s und eine bewusste Entscheidung des Betreibers."]),

 ("Anwendungsfall 5 — Plattform für Wellenformen", [
   "Wer eine neue Wellenform ausprobieren will, will nicht erst ein Modem schreiben.",
   "",
   "Was OpenPulseHF dafür mitbringt:",
   "• Eine Plugin-Schnittstelle: eine Betriebsart ist ein Crate, kein Fork",
   "• Kanalsimulation mit Watterson-Schwund und Gilbert-Elliott-Bündelfehlern",
   "• Testbench mit Wasserfall, Spektrum und Konstellation an vier Messpunkten",
   "• Eine Testmatrix, die Betriebsart gegen Kanal automatisch durchmisst",
   "",
   "Der Nutzen ist der Vergleich: dieselbe Kette, derselbe Kanal, dieselbe Messung."]),

 ("Anwendungsfall 6 — ein Programm statt vieler", [
   "Eine typische KW-Datenstation betreibt mehrere getrennte Programme. OpenPulseHF ist",
   "eines, das das meiste davon abdeckt — mit denselben Protokollen nach außen.",
   "",
   "ERSETZT DURCH DEN DAEMON:",
   "• Soundkarten-TNC — ARDOP- und KISS/AX.25-Ports, Pat verbindet sich unverändert",
   "• Das Modem selbst — 16 Wellenform-Plugins, BPSK31 bis 64QAM, OFDM, SC-FDMA",
   "• Separates PTT-Werkzeug — seriell, VOX, rigctld/CAT, CM108 und GPIO eingebaut",
   "• Separate Transceiversteuerung — Frequenz und Betriebsart über CAT",
   "• Winlink-Transport — B2F-Sessiontreiber und direktes CMS-Gateway",
   "• Digipeater oder Relais — dasselbe Programm mit einem anderen Schalter",
   "",
   "WEITERHIN NÖTIG: Funkgerät, Audio-Interface, und ein Winlink-Client für das Postfach.",
   "KEIN ERSATZ FÜR: PACTOR-Hardware oder VARA — geschlossenes Protokoll, keine Kopplung."]),

 ("Anwendungsfall 7 — über eine dritte Station", [
   "Zwei Stationen ohne direkte Verbindung können über eine dritte arbeiten.",
   "",
   "• Wegbewertung ist vertrauensgewichtet und nutzt den SCHWÄCHSTEN Sprung,",
   "  nicht den Durchschnitt — ein schwaches Glied wird nicht weggemittelt",
   "• Jeder Sprung prüft die Ursprungssignatur: ein Relais kann den Absender",
   "  nicht fälschen, für den es zu tragen vorgibt",
   "• Duplikate werden unterdrückt, damit eine Schleife stirbt statt sich zu verstärken",
   "",
   "GENAU HINSEHEN: Das leitet in Echtzeit weiter. Es gibt KEIN Store-and-Forward —",
   "keine Warteschlange, keine Persistenz, keine Zustellung an eine Station, die gerade",
   "nicht auf Sendung ist. Fehlt der nächste Sprung, wird der Frame verworfen."]),

 ("Anwendungsfall 8 — Cross-Band-Relais", [
   "Eine Station überbrückt zwei Bänder: Empfang auf dem einen, Aussendung auf dem anderen.",
   "Nützlich, wenn eine Gruppe zwischen KW-Runde und lokaler UKW-Frequenz geteilt ist.",
   "",
   "• Läuft vollduplex in einem eigenen Thread, zur Laufzeit ein- und ausschaltbar",
   "• Filter und Weiterleitungsregeln entscheiden, was überhaupt wiederholt wird",
   "",
   "VERANTWORTUNG DES BETREIBERS: Ein automatisch getasteter Sender ist genehmigungs-",
   "relevant. Rufzeichennennung, Bandzuweisung und die Regeln für unbeaufsichtigten",
   "Betrieb liegen bei Ihnen — die Software kennt weder Ihre Klasse noch Ihr Land."]),

 ("Anwendungsfall 9 — ein Netz aus Baken", [
   "Mesh-Knoten wiederholen signierte Baken mit abnehmender Lebensdauer, sodass eine",
   "Station mehrere Sprünge entfernt erfährt, dass es uns gibt — ohne zentralen Server.",
   "",
   "• Baken sind Ed25519-signiert und selbst-authentifizierend: die Peer-ID IST",
   "  der Prüfschlüssel, es braucht keinen Schlüsselserver",
   "• Die Lebensdauer begrenzt die Flut, Duplikaterkennung stoppt Schleifen",
   "• Das Gelernte fließt in den gemeinsamen Peer-Cache für Abfragen und Wege",
   "",
   "RAHMEN: Verteilt wird ERREICHBARKEITSINFORMATION. Das ist kein datentragendes Mesh,",
   "und eine Bake belegt, dass ein Weg bestand — nicht, dass er jetzt besteht."]),

 ("Anwendungsfall 10 — weg von schlechter Frequenz", [
   "Eine Frequenz, die vor zehn Minuten gut war, kann durch ein neues Signal oder eine",
   "geänderte Ausbreitung unbrauchbar werden.",
   "",
   "• Der Scanner misst Kandidatenfrequenzen und verweilt auf jeder, der Vorschlag",
   "  beruht also auf Gehörtem statt auf einer Vermutung",
   "• Der Austausch ist signiert: Dritte können die Verbindung nicht auf eine",
   "  Frequenz ihrer Wahl umleiten",
   "• Beide Seiten wechseln zu einem vereinbarten Zeitpunkt — geplant, nicht gerannt",
   "",
   "EHRLICHER RAHMEN: Zustandsautomat, Format und Scan sind fertig und getestet; der",
   "Wechsel wird ausdrücklich vorgeschlagen und angenommen, nicht automatisch ausgelöst."]),

 ("Anwendungsfall 11 — wer auf FreeDV spricht", [
   "FreeDV überträgt digitale Sprache, und eine vertraute Stimme ist kein Identitätsnachweis.",
   "",
   "• Läuft NEBEN FreeDV über dessen UDP-Datenkanal — FreeDV wird nicht verändert",
   "• Jede Bake signiert Rufzeichen, Zeitstempel, Nonce, Frequenz und Betriebsart,",
   "  sodass eine Aufzeichnung nicht auf eine andere Frequenz oder Zeit passt",
   "",
   "WAS ES NICHT IST: Es authentifiziert nicht das AUDIO, sondern eine Bake daneben.",
   "Wer die Sprache weiterreicht und eigene Baken sendet, wird damit nicht erkannt.",
   "Die belastbare Aussage lautet: diese Station ist auf Frequenz und signiert."]),
]

CONTENT_SLIDES = [
 ("Agenda", [
   "1.  Anwendungsfälle — elf Dinge, die man mit einer KW-Datenverbindung tut",
   "2.  Was OpenPulseHF ist — und welches Problem es adressiert",
   "3.  Was es von anderen Software- und Hardware-Modems unterscheidet",
   "4.  Wie es entwickelt und implementiert wird",
   "5.  Wie es getestet wird — die Ebenen, und was jede belegen kann",
   "6.  Funktionsumfang, fortgeschrittene Funktionen, gemessene Leistung",
   "7.  Stand und Ausblick — einschließlich dessen, was NICHT fertig ist"]),

 ("Das Problem: Daten über Kurzwelle", [
   "Kurzwelle bietet interkontinentale Reichweite ohne Infrastruktur — und einen",
   "feindseligen Kanal.",
   "",
   "• Mehrwegeausbreitung → Laufzeitstreuung und frequenzselektiver Schwund",
   "• Doppler → die Phasenreferenz des Trägers wandert unter einem weg",
   "• Bündelfehler durch atmosphärische Störungen und Kurzzeitschwund",
   "• Sehr begrenzte Bandbreite: typisch 2,4 bis 2,7 kHz",
   "• Zwei Taktquellen, die nie exakt übereinstimmen",
   "",
   "Ein Modem, das im Labor funktioniert und auf dem Band nicht, hat keinen Fehler",
   "im Labor — es hat einen Kanal, den das Labor nicht kannte."]),

 ("Was OpenPulseHF ist", [
   "Ein offenes, Plugin-basiertes Software-Modem für Kurzwelle, in Rust geschrieben.",
   "",
   "• 16 Wellenform-Plugins von BPSK31 bis 64QAM, dazu OFDM und SC-FDMA",
   "• Ein adaptiver Ratenmechanismus, der auf Dekodierbelegen aufsteigt",
   "• Signierte Identität im Funkkanal, klassisch und post-quantum",
   "• Winlink, KISS/AX.25 und ARDOP nach außen — vorhandene Clients passen",
   "• Läuft auf einem Raspberry Pi ebenso wie auf einem Arbeitsplatzrechner",
   "",
   "Alles offen: Quelltext, Wellenformspezifikation, Entwurfsdokumente und das",
   "Änderungsjournal. Wer mithören will, kann jede Aussendung nachvollziehen."]),

 ("Wo es sitzt", ["Ein Programm, sieben Ebenen — und genau eine davon ist eine Plugin-Grenze."]),

 ("Unterschied 1 — Wellenformen sind Plugins", [
   "In den meisten Modems ist die Betriebsart einkompiliert. Hier ist sie ein Crate,",
   "das eine Schnittstelle erfüllt.",
   "",
   "• Modulator, Demodulator, optional weiche Entscheidungswerte, Präambel",
   "• Zur Laufzeit registriert und auswählbar — kein Neubau für eine neue Betriebsart",
   "• Dieselbe Testmatrix misst jedes Plugin gegen dieselben Kanäle",
   "",
   "Der praktische Effekt: Eine neue Wellenform vergleicht sich sofort mit allen",
   "vorhandenen, unter identischen Bedingungen. Das ist der eigentliche Wert."]),

 ("Unterschied 2 — die Leiter steigt nach BELEGEN",
  ["Vierzehn Stufen. Sie steigt nach Dekodierungen, nicht nach einer Schätzung."]),

 ("Unterschied 3 — signierte Identität im Funk", [
   "Wer sendet, behauptet ein Rufzeichen. Auf Kurzwelle prüft das üblicherweise niemand.",
   "",
   "• Ed25519-signierter Verbindungsaufbau, vollständig im Funkkanal",
   "• Post-quantum-Verfahren vorbereitet (ML-DSA-44, ML-KEM-768), Format fertig",
   "• Jede signierende Stelle ist an eine registrierte Domäne gebunden, damit eine",
   "  Signatur aus einem Kontext nicht in einem anderen gilt",
   "• Der Rahmen passt in ein einziges Fragment — auf einem schwundbehafteten",
   "  Kanal ist das der Unterschied zwischen p und p³",
   "",
   "KEINE VERSCHLÜSSELUNG: Der Inhalt bleibt im Klartext, wie es das Amateurfunkrecht",
   "verlangt. Signiert wird die Identität, nicht der Inhalt."]),

 ("Implementierung — die Form des Codes", [
   "Rust, ein Workspace aus rund 30 Crates.",
   "",
   "• Kern: Frame-Format, FEC, Zustandsautomat, Vertrauen, Segmentierung",
   "• Modem: Ablaufsteuerung, Kanalzugriff, Diagnose, Kanalsimulation",
   "• DSP: RRC-Filter, PLL, Gardner-Taktrückgewinnung, adaptiver Entzerrer",
   "• Protokolle: ARDOP, KISS, B2F, Dateitransfer, QSY, Discovery, Mesh",
   "• Oberflächen: CLI, TUI, Bedienpanel, Testbench, Zwillingsansicht",
   "",
   "Keine unwrap-Aufrufe in produktiven Bibliothekspfaden. Fehler werden typisiert",
   "weitergereicht, nicht verschluckt."]),

 ("Implementierung — eine Nahtstelle, nicht viele", [
   "Empfangenes Audio erreicht den Demodulator über zwei verschiedene Wege.",
   "",
   "Eine Front-End-Verarbeitung, die an EINEM der Aufrufer hängt, läuft auf dem",
   "anderen nie — und genau das ist einmal passiert: Das Empfangs-Notchfilter saß",
   "in der Testfunktion und lief im Daemon nicht.",
   "",
   "Konsequenz: Solche Transformationen sitzen an der einen gemeinsamen Nahtstelle,",
   "und ein Zähler belegt zur Laufzeit, dass sie dort auch ausgeführt werden.",
   "",
   "Aus einem Callers-Grep folgt kein Abdeckungsnachweis. Nur ein Test, der ohne",
   "die Verdrahtung fehlschlägt, belegt sie."]),

 ("Wie entwickelt wird — Anforderungen als DATEN", [
   "Anforderungen liegen als maschinenlesbare Datei vor, nicht nur als Prosa.",
   "",
   "• Jede Anforderung ist einer Fähigkeit zugeordnet, jede Fähigkeit dem Code",
   "• Ein Gate prüft die Zuordnung in beide Richtungen und meldet Verwaisungen",
   "• Commits tragen die Anforderung, der sie dienen, als Trailer",
   "• Ein Journal hält die Kette fest: Anforderung → Entwurf → Code → Test → Ergebnis",
   "",
   "Der Zweck ist nicht Bürokratie, sondern Auffindbarkeit: Zu jeder Zeile lässt",
   "sich fragen, wofür sie da ist — und die Antwort ist überprüfbar."]),

 ("Wie entwickelt wird — gegnerische Prüfung", [
   "Jede Entwurfsentscheidung und jede Schlussfolgerung wird von einem zweiten",
   "Modell geprüft, BEVOR sie in Code oder Dokumentation eingeht.",
   "",
   "Der Grund ist erfahrungsbasiert: Die teuren Fehler in diesem Projekt waren nie",
   "schlechter Code, sondern zuversichtliche falsche Überzeugungen, die lange genug",
   "unbemerkt blieben, um darauf aufzubauen.",
   "",
   "Beispiele aus der Praxis: ein Gate-Ergebnis, das aus dem Cache stammte und für",
   "den Baum nicht galt; eine handgepflegte Liste, die im selben Commit veraltete,",
   "der Listenpflege kritisierte; drei Zahlen, die ihren Quellen widersprachen."]),

 ("Testen — sechs Ebenen",
  ["Jede Ebene ist billiger als die darüber — und belegt weniger. Beides zählt."]),

 ("Testen — was jede Ebene NICHT belegen kann", [
   "Die nützlichere Hälfte der Tabelle steht rechts, nicht links.",
   "",
   "• Ein Unit-Test belegt nichts über die Luftschnittstelle",
   "• Ein Integrationstest reicht den Puffer als Frame — es gibt nichts zu suchen",
   "• Der Kanalsimulator ist ein Modell, nicht die Ionosphäre",
   "• Der virtuelle Loopback hat eine Taktquelle und sieht keinen Ratenversatz",
   "• Das Zwei-Karten-Rig hat einen Gerätezustand, der sich unbemerkt ändert",
   "",
   "Ein Fehler, der auf Rig A auftritt und auf Rig B nicht, isoliert nur dann eine",
   "Ursache, wenn alles andere wirklich gleich ist. Gerätezustand ist das selten."]),

 ("Testen — das Gate", [
   "Ein Befehl ist maßgeblich. Er läuft ohne Pipes und schreibt ein Ergebnis.",
   "",
   "• Formatierung, Lint mit Warnungen als Fehler, gesamte Testsuite",
   "• Anforderungs- und Erreichbarkeitsprüfung, Journal-Reihenfolge",
   "• Prüfung, dass jede Änderung eine gegnerische Prüfung ausweist",
   "",
   "Stand: 316 Suites · 2 408 Tests · 0 Fehler · 8 Schritte",
   "",
   "Regel im Projekt: Eine Aussage über Bestehen oder Scheitern stammt aus der",
   "Ergebniszeile dieses Befehls — nie aus dem Exit-Code hinter einer Pipe."]),

 ("Funktionsumfang", [
   "• 85 Betriebsarten aus 10 Wellenformfamilien, zur Laufzeit wählbar",
   "• Adaptive Ratenleiter mit ARQ und weicher Kombination über Wiederholungen",
   "• Reed-Solomon, Faltungscode und LDPC, je nach Stufe und Nutzlast",
   "• Winlink über Funk und direktes CMS-Gateway über TCP",
   "• KISS/AX.25 und ARDOP für vorhandene Clients",
   "• Automatische Rufzeichennennung in Sprache des Betriebs und als CW",
   "• Sicheres Ablegen von Schlüsseln, verschlüsselter Steuerkanal",
   "• ADIF-Logbuch, auf Wunsch automatisch pro Verbindung"]),

 ("Fortgeschrittene Funktionen", [
   "• Winlink/B2F: vollständiger Sessiontreiber, direktes CMS-Gateway",
   "• Dateitransfer mit Wiederaufnahme, Blockbestätigung und signiertem Manifest",
   "• QSY: Frequenzwechsel im Einvernehmen, signiert und zeitlich geplant",
   "• Discovery über JS8 mit Bake, Stationstabelle und Rendezvous",
   "• Mesh: Baken mit Lebensdauer, Relais mit Vertrauensbewertung",
   "• Cross-Band-Relais und Digipeater im selben Programm",
   "• GPU-Beschleunigung für ausgewählte DSP-Kernel, mit CPU-Rückfall",
   "• Zwillings-Rig: zwei echte Daemons, im Loopback gekoppelt und beobachtbar"]),

 ("Leistung — im Kanalsimulator gemessen",
  ["Dekodierrate bei Watterson moderate_f1. Die Nullen sind das Interessante."]),

 ("Leistung — ein Ergebnis im Detail", [
   "Der signierte Verbindungsaufbau war 752 Byte groß — drei Fragmente.",
   "",
   "Auf einem schwundbehafteten Kanal überlebt ein Austausch aus drei Fragmenten",
   "mit etwa p³ statt p. Bei p = 0,7 sind das 34 % statt 70 %.",
   "",
   "Die Umstellung auf ein binäres Format brachte ihn auf 187 Byte: ein Fragment.",
   "",
   "Der Punkt ist nicht die Ersparnis an Bytes. Der Punkt ist, dass die",
   "Fehlerwahrscheinlichkeit von einer Potenz auf eine lineare Größe fällt —",
   "und dass das erst sichtbar wurde, als jemand nach Fragmenten statt nach",
   "Sekunden gefragt hat."]),

 ("Regulatorik — durchgesetzt und dokumentiert", [
   "IM CODE DURCHGESETZT",
   "• Rufzeichennennung nach Zeit und am Ende einer Aussendungsfolge",
   "• CW-Kennung als Audio — für jede empfangende Station lesbar, nicht nur für uns",
   "• Relais-, Antwort- und QSY-Pfade tasten ohne Rufzeichen nicht",
   "• Sendepausen sind zeitlich begrenzt, und das Format ist vollständig publiziert",
   "",
   "DOKUMENTIERT — UND BEIM BETREIBER BELASSEN",
   "• Symbolrate je Band — BPSK31/63/100/250 unter 300 Baud, QPSK500+ nicht",
   "• Bandbreite, Leistung, Bandsegmente — je nach Verwaltung und Klasse verschieden",
   "• Automatische Betriebsstelle für unbeaufsichtigten Relaisbetrieb",
   "• IARU-Bandpläne — Empfehlung statt Gesetz, aber weithin beachtet",
   "",
   "Das Modem erzeugt auch eine Betriebsart, die auf Ihrer Frequenz unzulässig ist —",
   "eine Entscheidung, kein Versehen: Zulässigkeit hängt von Band, Land und Klasse ab.",
   "Gesichtet: FCC 97 · CEPT/ECC · BNetzA (AFuV) · Ofcom · IARU. On-Air: aufgeschoben."]),

 ("Stand — und was NICHT fertig ist", [
   "v0.16.0, vor 1.0 · 2 408 Tests grün · über 360 Journaleinträge",
   "",
   "FERTIG: Wellenformen, Ratenleiter, ARQ, Winlink, Dateitransfer, Discovery,",
   "Mesh, QSY, signierter Verbindungsaufbau, Bedienoberflächen.",
   "",
   "NICHT FERTIG, und das gehört auf diese Folie:",
   "• Regulatorische Abnahme auf der Luftschnittstelle ist aufgeschoben",
   "• Die Zeit zwischen letztem Sample und PTT-Abfall ist ungemessen — braucht ein Rig",
   "• Die Stufengrenzen stammen aus Simulation, nicht aus dem Bandbetrieb",
   "",
   "Ein Projekt, das mit 2 408 grünen Tests eröffnet, suggeriert mehr als das.",
   "Deshalb steht es hier ausdrücklich."]),

 ("Ausblick", [
   "Vor 1.0:",
   "• Das Zeitfenster für Formatänderungen schließen — danach wird jede teuer",
   "• Die Empfangskette dort verdrahten, wo der Daemon sie tatsächlich ausführt",
   "• Betrieb auf der Luftschnittstelle als harte Voraussetzung, nicht als Wunsch",
   "",
   "Danach:",
   "• Breitere Kanäle, wo sie zulässig sind",
   "• Weitere Wellenformen — die Plugin-Grenze macht das billig",
   "",
   "Der Leitsatz bleibt: Eine Dekodierung ist eine Beobachtung, das SNR ein Modell.",
   "Wenn beide sich widersprechen, gewinnt die Beobachtung."]),
]
