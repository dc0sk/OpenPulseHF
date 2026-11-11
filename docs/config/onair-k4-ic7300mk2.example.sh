# On-air HF profile for scripts/run-onair-twin-ota.sh: Elecraft K4 (station A) and Icom IC-7300MK2
# (station B), the first HF pair (decision 28; docs/dev/onair-hf-plan.md). Copy, fill in, and source it:
#   cp docs/config/onair-k4-ic7300mk2.example.sh ~/onair-hf.sh && $EDITOR ~/onair-hf.sh
#   source ~/onair-hf.sh && scripts/run-onair-twin-ota.sh check
# Everything marked UNCHECKED is to be read at the stations before the first run. No host details go into
# the repository: the ssh targets stay in your local copy.

export A_LABEL="K4"
export B_LABEL="IC-7300MK2"
export A_SSH="FILL IN"                    # station A's controller (user@host), local copy only
export B_SSH="FILL IN"                    # station B's controller
export CALLSIGN_A="FILL IN"; export GRID_A="FILL IN"
export CALLSIGN_B="FILL IN"; export GRID_B="FILL IN"

# CAT (Hamlib models from `rigctl -l`: K4 2047, IC-7300 3073; the MK2 answering as 3073 is UNCHECKED).
export A_HAMLIB_MODEL=2047
export A_CAT_PORT="FILL IN"               # the K4's USB CAT port (UNCHECKED: USB or LAN)
export A_CAT_BAUD=38400                   # UNCHECKED
export B_HAMLIB_MODEL=3073
export B_CAT_PORT="FILL IN"               # the IC-7300MK2's USB CI-V port
export B_CAT_BAUD=115200                  # UNCHECKED; the CI-V address too (rigctl -C civaddr=...)
# PTT over CAT on both: then `check` is keyless (an RTS/DTR PTT line can assert when rigctld opens it).
export A_PTT_TYPE=CAT; export B_PTT_TYPE=CAT

# Audio: the rigs' USB codecs. Measure the device names and the RX/TX levels at the stations, as for the
# 2 m pair (onair-setup-audio-routing.sh).
export A_AUDIO_DEVICE="FILL IN"; export B_AUDIO_DEVICE="FILL IN"

# Band and segment: pick per onair-hf-plan.md. The example is 20 m, `fast` profile, in the wide-digital
# segment ABOVE the IBP beacons (14.099-14.101 MHz, NCDXF at 14.100): never start a segment at 14.099.
export OTA_PROFILE=fast
export TEST_FREQ_HZ=14104000              # dial; the signal sits at dial + 0..3000 Hz (USB)
export BAND_MIN_HZ=14101000; export BAND_MAX_HZ=14112000
export A_FREQ_OFFSET_HZ=0; export B_FREQ_OFFSET_HZ=0   # measure each rig's offset on the band
export A_DISPLAY_SHIFT_HZ=0; export B_DISPLAY_SHIFT_HZ=0
# The rigs' TX audio passband, which bounds the emission (the OFDM clipping regrows out of band):
# IC-7300 TBW WIDE is about 100-2900 Hz (UNCHECKED on the MK2); the K4's TX filter is UNCHECKED.
export A_TX_LO_HZ=0; export A_TX_HI_HZ=3000
export B_TX_LO_HZ=0; export B_TX_HI_HZ=3000

# Mode: both backends name the data mode PKTUSB. The IC-7300MK2's plain USB keys the microphone.
export A_TEST_MODE_RIG=PKTUSB; export B_TEST_MODE_RIG=PKTUSB
export A_REQUIRE_DATA_MODE=1; export B_REQUIRE_DATA_MODE=1
export RIG_PASSBAND_HZ=3000               # sent with the mode
export A_PASSBAND_EXPECT=2400; export B_PASSBAND_EXPECT=2400

# AGC FAST (Hamlib 2) on both receivers, a Release 1 operating requirement. Set it on the rigs; the
# runner only reads it and refuses on anything else.
export A_EXPECT_AGC=2; export B_EXPECT_AGC=2

# Power: Hamlib's 0..1 fraction, only ever LOWERED by the runner. On a 100 W rig 0.10 is 10 W. Start
# there; raise it only by hand on the rig, and only if the link needs it (minimum necessary power).
export A_RFPOWER=0.10; export B_RFPOWER=0.10
# The integrated tuner: the operator tunes on the rig before the run. The daemon's
# `allow_integrated_tuner_on_high_swr` stays at its default (off); this runner does not set it.
