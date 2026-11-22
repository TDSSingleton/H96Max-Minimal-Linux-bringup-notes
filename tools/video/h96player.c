/* h96player -- the H96Max stream player that h96ui runs (NOTES §34-§36).
 *
 *   h96player [-i] [-u user-agent] [-r referer] [-H 'Name: value']... [-b buffer-seconds] [-L low%]
 *             [-p plane-id] [-c connector-id] [-a alsa-device] [-C channels] [-v volume 0..1] URI
 *
 * playbin3 with the hardware path: mppvideodec (VPU, picked by rank) -> NV12 dmabuf -> kmssink on
 * a VOP2 plane above the console (hardware scaling); audio to ALSA. Over gst-launch it adds:
 *   - -C N (default 2): audio converted to N channels before ALSA. HDMI refuses a channel count
 *     the TV's speaker allocation does not list ("hdmi-audio-codec: Not able to map channels to
 *     speakers", EINVAL from hw_params -> alsasink "Could not get/set settings"); plughw passes
 *     5.1 straight through, so a 5.1 film on a stereo TV failed. -C 0 = as the source.
 *   - HTTP request headers from the playlist (http-user-agent / http-referrer / Kodi "|Header=")
 *     set on the source in "source-setup". hlsdemux2 copies User-Agent and Referer from the
 *     source's request onto every playlist and segment fetch.
 *   - a prebuffer: hlsdemux2's high watermark = -b seconds, and the pipeline stays PAUSED while
 *     the demuxer reports buffering < 100% (at start and after a stall), so a slow server shows
 *     as one pause instead of constant stutter.
 *   - hysteresis: once playing it pauses again only when the level drops below -L percent
 *     (hlsdemux2 reports its level continuously, so "< 100%" alone would pause all the time).
 *   - -i: control from stdin, one command per line (h96ui's remote keys):
 *       toggle | pause | play          pause/resume (VOD; also works on live, it just catches up)
 *       seek <+-seconds>               relative seek (if seekable)
 *       volume <0..1> | mute <0|1>     audio level (playbin's software volume)
 *       rect <x> <y> <w> <h> | full    video rectangle in display pixels (the "OSD": h96ui
 *                                      shrinks the video and draws text on the console around it)
 *     and "h96player: status pos=<s> dur=<s|-1> seekable=<0|1> paused=<0|1>" once a second.
 *     EOF on stdin (h96ui gone) stops playback.
 *   - one-line, parseable status on stderr: "h96player: error: <text>", "h96player: buffering N%",
 *     "h96player: playing". Exit 0 at end of stream, 1 on error, 0 on SIGTERM/SIGINT (clean
 *     NULL state, so the display plane is released before h96ui redraws the console).
 */
#include <glib-unix.h>
#include <gst/gst.h>
#include <gst/video/videooverlay.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>

static struct {
  const char *ua, *referer, *plane, *connector, *alsa;
  GPtrArray *headers; /* "Name: value" */
  double volume;
  int channels; /* 0: as the source */
  guint64 buffer_ns;
  int low_pct;
} opt = { NULL, NULL, "172", "371", "plughw:CARD=rockchiphdmi,DEV=0", NULL, 0.8, 2, 5 * GST_SECOND, 2 };

static gint64 t_start;
static GstElement *pipe_;
static void quit (void);
static gboolean seeking = FALSE;      /* a flushing seek is in flight (until ASYNC_DONE) */
static gboolean seek_pending = FALSE;
static double seek_pending_s = 0;

/* Every runtime state change goes through call_async: a state change racing a flushing seek
 * deadlocked inside the pipeline (NOTES §37) and froze the main loop -- status, commands and
 * even SIGTERM. The main loop must never block. */
static void do_set_state (GstElement * e, gpointer state)
{
  gst_element_set_state (e, (GstState) GPOINTER_TO_INT (state));
}

static void set_state_async (GstState st)
{
  gst_element_call_async (pipe_, do_set_state, GINT_TO_POINTER (st), NULL);
}
static gboolean trace;

/* status line on stderr; with H96PLAYER_TRACE=1 prefixed by seconds since start */
static void say (const char *fmt, ...) G_GNUC_PRINTF (1, 2);
static void say (const char *fmt, ...)
{
  va_list ap;
  if (trace)
    fprintf (stderr, "%6.1f ", (g_get_monotonic_time () - t_start) / 1e6);
  fputs ("h96player: ", stderr);
  va_start (ap, fmt);
  vfprintf (stderr, fmt, ap);
  va_end (ap);
  fputc ('\n', stderr);
}

static GMainLoop *loop;
static GstElement *pipe_;
static int exit_code = 0;
static gboolean buffering = TRUE, playing = FALSE;   /* prerolling counts as buffering */
static gboolean user_paused = FALSE, interactive = FALSE;
static GstElement *vsink;
static GString *inbuf;
static int last_pct = -1;

static void on_source_setup (GstElement * playbin, GstElement * src, gpointer u)
{
  GObjectClass *k = G_OBJECT_GET_CLASS (src);
  if (opt.ua && g_object_class_find_property (k, "user-agent"))
    g_object_set (src, "user-agent", opt.ua, NULL);
  if (g_object_class_find_property (k, "extra-headers") && (opt.referer || opt.headers->len)) {
    GstStructure *h = gst_structure_new_empty ("extra-headers");
    if (opt.referer)
      gst_structure_set (h, "Referer", G_TYPE_STRING, opt.referer, NULL);
    for (guint i = 0; i < opt.headers->len; i++) {
      char *nv = g_strdup (g_ptr_array_index (opt.headers, i)), *c = strchr (nv, ':');
      if (c) {
        *c = 0;
        gst_structure_set (h, g_strstrip (nv), G_TYPE_STRING, g_strstrip (c + 1), NULL);
      }
      g_free (nv);
    }
    g_object_set (src, "extra-headers", h, NULL);
    gst_structure_free (h);
  }
  /* live IPTV servers can be slow to answer the first request */
  if (g_object_class_find_property (k, "timeout"))
    g_object_set (src, "timeout", 20u, NULL);
}

/* hlsdemux2 / dashdemux2: buffer opt.buffer_ns before reporting 100% */
static void on_element_setup (GstElement * playbin, GstElement * e, gpointer u)
{
  GObjectClass *k = G_OBJECT_GET_CLASS (e);
  if (g_object_class_find_property (k, "high-watermark-time")) {
    guint64 maxb = 0;
    g_object_get (e, "max-buffering-time", &maxb, NULL);
    if (maxb && maxb < opt.buffer_ns)
      g_object_set (e, "max-buffering-time", opt.buffer_ns, NULL);
    g_object_set (e, "high-watermark-time", opt.buffer_ns, NULL);
  }
}

static void start_seek (double delta_s);

/* The useful part of a GStreamer debug string: its last line ("file.c(123): func (): /path/
 * to/element:\nRate 44100Hz not available for playback: Invalid argument" -> the reason). */
static gchar *dbg_reason (const gchar * dbg)
{
  if (!dbg || !*dbg)
    return NULL;
  gchar *d = g_strstrip (g_strdup (dbg));
  const gchar *nl = strrchr (d, '\n');
  gchar *r = g_strdup (nl ? nl + 1 : d);
  g_free (d);
  g_strdelimit (r, "\r\t", ' ');
  return r;
}

/* error/warning line: "<message> [<element>: <reason>]". The element and the debug reason are
 * what tells "Could not get/set settings from/on resource" (no text of its own: alsasink,
 * rtspsrc, ...) apart; the full debug string goes on a "detail:" line for the log. */
static void say_problem (const char *kind, GstMessage * m, GError * err, gchar * dbg)
{
  const gchar *src = GST_MESSAGE_SRC_NAME (m) ? GST_MESSAGE_SRC_NAME (m) : "?";
  gchar *why = dbg_reason (dbg);
  if (why && !strstr (err ? err->message : "", why))
    say ("%s: %s [%s: %s]", kind, err ? err->message : "unknown", src, why);
  else
    say ("%s: %s [%s]", kind, err ? err->message : "unknown", src);
  if (dbg && *dbg) {
    gchar *flat = g_strdup (dbg);
    g_strdelimit (flat, "\r\n\t", ' ');
    say ("detail: %s", flat);
    g_free (flat);
  }
  g_free (why);
}

static gboolean on_bus (GstBus * bus, GstMessage * m, gpointer u)
{
  switch (GST_MESSAGE_TYPE (m)) {
    case GST_MESSAGE_ERROR:{
      GError *err = NULL;
      gchar *dbg = NULL;
      gst_message_parse_error (m, &err, &dbg);
      say_problem ("error", m, err, dbg);
      g_clear_error (&err);
      g_free (dbg);
      exit_code = 1;
      g_main_loop_quit (loop);
      break;
    }
    case GST_MESSAGE_WARNING:{
      GError *err = NULL;
      gchar *dbg = NULL;
      gst_message_parse_warning (m, &err, &dbg);
      say_problem ("warning", m, err, dbg);
      g_clear_error (&err);
      g_free (dbg);
      break;
    }
    case GST_MESSAGE_EOS:
      say ("end of stream");
      g_main_loop_quit (loop);
      break;
    case GST_MESSAGE_BUFFERING:{
      gint pct = 0;
      gst_message_parse_buffering (m, &pct);
      if (!buffering && !seeking && pct < opt.low_pct) {
        buffering = TRUE;               /* starved: refill to 100% before resuming */
        say ("stalled, rebuffering");
        set_state_async (GST_STATE_PAUSED);
      } else if (buffering && pct >= 100) {
        buffering = FALSE;
        if (!user_paused)
          set_state_async (GST_STATE_PLAYING);
      }
      if (pct / 10 != last_pct / 10 || pct == 100)
        say ("buffering %d%%", pct);
      last_pct = pct;
      break;
    }
    case GST_MESSAGE_STATE_CHANGED:
      if (GST_MESSAGE_SRC (m) == GST_OBJECT (pipe_)) {
        GstState news;
        gst_message_parse_state_changed (m, NULL, &news, NULL);
        if (news == GST_STATE_PLAYING && !playing) {
          playing = TRUE;
          say ("playing");
        }
      }
      break;
    case GST_MESSAGE_ASYNC_DONE:
      /* prerolled without a single buffering message (file, progressive HTTP): just play */
      if (buffering && last_pct < 0) {
        buffering = FALSE;
        set_state_async (GST_STATE_PLAYING);
      }
      if (seeking) {                    /* the flushing seek completed */
        seeking = FALSE;
        if (seek_pending) {
          seek_pending = FALSE;
          start_seek (seek_pending_s);
        }
      }
      break;
    case GST_MESSAGE_CLOCK_LOST:
      set_state_async (GST_STATE_PAUSED);
      set_state_async (GST_STATE_PLAYING);
      break;
    default:
      break;
  }
  return TRUE;
}

/* trace mode: rendered/dropped video frames every 5 s (frozen picture = rendered not growing) */
static gboolean on_stats (gpointer vs)
{
  GstStructure *st = NULL;
  guint64 rendered = 0, dropped = 0;
  g_object_get (vs, "stats", &st, NULL);
  if (st) {
    gst_structure_get_uint64 (st, "rendered", &rendered);
    gst_structure_get_uint64 (st, "dropped", &dropped);
    gst_structure_free (st);
  }
  say ("frames rendered %" G_GUINT64_FORMAT " dropped %" G_GUINT64_FORMAT, rendered, dropped);
  return G_SOURCE_CONTINUE;
}

static gint64 query_ns (gboolean duration)
{
  gint64 v = -1;
  if (!(duration ? gst_element_query_duration (pipe_, GST_FORMAT_TIME, &v)
          : gst_element_query_position (pipe_, GST_FORMAT_TIME, &v)))
    return -1;
  return v;
}

static gboolean is_seekable (void)
{
  gboolean seekable = FALSE;
  GstQuery *q = gst_query_new_seeking (GST_FORMAT_TIME);
  if (gst_element_query (pipe_, q))
    gst_query_parse_seeking (q, NULL, &seekable, NULL, NULL);
  gst_query_unref (q);
  return seekable;
}

/* Status for h96ui, without wedging the main loop: nothing at all while a seek is in flight;
 * position from the video sink (basesink answers it locally from its segment and clock);
 * duration and seekability asked of the pipeline only until known, then cached. */
static gint64 seek_started;
static gint64 dur_cache = -1;
static int seekable_cache = -1;
static double last_pos = 0;

static gboolean on_status (gpointer u)
{
  if (seeking && g_get_monotonic_time () - seek_started > 5 * G_USEC_PER_SEC)
    seeking = FALSE;            /* no ASYNC_DONE for this seek: do not freeze the status */
  if (!seeking && playing) {
    gint64 pos = -1;
    if (gst_element_query_position (vsink, GST_FORMAT_TIME, &pos) && pos >= 0)
      last_pos = pos / 1e9;
    if (dur_cache < 0)
      dur_cache = query_ns (TRUE);
    if (seekable_cache < 0 && dur_cache > 0)
      seekable_cache = is_seekable ();
  }
  say ("status pos=%.1f dur=%.1f seekable=%d paused=%d", last_pos,
      dur_cache < 0 ? -1.0 : dur_cache / 1e9, seekable_cache == 1, user_paused);
  return G_SOURCE_CONTINUE;
}

static void set_rect (gint x, gint y, gint w, gint h)
{
  if (!GST_IS_VIDEO_OVERLAY (vsink))
    return;
  gst_video_overlay_set_render_rectangle (GST_VIDEO_OVERLAY (vsink), x, y, w, h);
  gst_video_overlay_expose (GST_VIDEO_OVERLAY (vsink));      /* redraw now, even when paused */
}

static void do_seek (GstElement * e, gpointer target)
{
  gst_element_seek_simple (e, GST_FORMAT_TIME, GST_SEEK_FLAG_FLUSH | GST_SEEK_FLAG_KEY_UNIT,
      *(gint64 *) target);
}

/* relative seek, clamped to [0, duration - 1 s], run off the main loop */
static void start_seek (double delta_s)
{
  if (seekable_cache != 1)
    return;
  gint64 *t = g_new (gint64, 1);
  *t = (gint64) ((last_pos + delta_s) * GST_SECOND);
  if (dur_cache > 0 && *t > dur_cache - GST_SECOND)
    *t = dur_cache - GST_SECOND;
  if (*t < 0)
    *t = 0;
  last_pos = *t / 1e9;          /* report the target until the sink knows better */
  seeking = TRUE;
  seek_started = g_get_monotonic_time ();
  seek_pending_s = 0;
  gst_element_call_async (pipe_, do_seek, t, g_free);
}

static void command (const char *line)
{
  gint x, y, w, h;
  double d;
  if (!strcmp (line, "toggle") || !strcmp (line, "pause") || !strcmp (line, "play")) {
    gboolean want = !strcmp (line, "toggle") ? !user_paused : !strcmp (line, "pause");
    user_paused = want;
    if (!buffering)
      set_state_async (want ? GST_STATE_PAUSED : GST_STATE_PLAYING);
  } else if (sscanf (line, "seek %lf", &d) == 1) {
    if (seeking) {                    /* one seek at a time: merge into the next one */
      seek_pending = TRUE;
      seek_pending_s += d;
    } else {
      seek_pending_s = 0;
      start_seek (d);
    }
  } else if (sscanf (line, "volume %lf", &d) == 1) {
    g_object_set (pipe_, "volume", CLAMP (d, 0.0, 1.0), NULL);
  } else if (sscanf (line, "mute %d", &x) == 1) {
    g_object_set (pipe_, "mute", x != 0, NULL);
  } else if (sscanf (line, "rect %d %d %d %d", &x, &y, &w, &h) == 4) {
    set_rect (x, y, w, h);
  } else if (!strcmp (line, "full") && GST_IS_VIDEO_OVERLAY (vsink)) {
    gint dw = 0, dh = 0;
    g_object_get (vsink, "display-width", &dw, "display-height", &dh, NULL);
    if (dw > 0 && dh > 0)
      set_rect (0, 0, dw, dh);
  } else if (*line) {
    say ("unknown command: %s", line);
  }
}

static gboolean on_stdin (gint fd, GIOCondition cond, gpointer u)
{
  char buf[256];
  ssize_t n = read (fd, buf, sizeof buf);
  if (n <= 0) {                 /* h96ui closed our stdin: it is gone or wants us gone */
    quit ();
    return G_SOURCE_REMOVE;
  }
  g_string_append_len (inbuf, buf, n);
  char *nl;
  while ((nl = strchr (inbuf->str, '\n'))) {
    *nl = 0;
    command (g_strstrip (inbuf->str));
    g_string_erase (inbuf, 0, nl - inbuf->str + 1);
  }
  return G_SOURCE_CONTINUE;
}

/* stopping must never hang (h96ui waits for us): if shutting the pipeline down takes more
 * than 2 s, leave anyway -- the kernel releases the display plane when the process exits */
static void on_alarm (int sig)
{
  _exit (exit_code);
}

static void quit (void)
{
  signal (SIGALRM, on_alarm);
  alarm (2);
  g_main_loop_quit (loop);
}

static gboolean on_signal (gpointer u)
{
  quit ();
  return G_SOURCE_REMOVE;
}

static GstElement *make (const char *factory)
{
  GstElement *e = gst_element_factory_make (factory, NULL);
  if (!e) {
    say ("error: GStreamer element %s is missing", factory);
    exit (1);
  }
  return e;
}

int main (int argc, char **argv)
{
  int c;
  opt.headers = g_ptr_array_new ();
  gst_init (&argc, &argv);
  t_start = g_get_monotonic_time ();
  trace = g_getenv ("H96PLAYER_TRACE") != NULL;
  while ((c = getopt (argc, argv, "iu:r:H:b:L:p:c:a:C:v:")) != -1) {
    switch (c) {
      case 'u': opt.ua = optarg; break;
      case 'r': opt.referer = optarg; break;
      case 'H': g_ptr_array_add (opt.headers, optarg); break;
      case 'b': opt.buffer_ns = (guint64) (g_ascii_strtod (optarg, NULL) * GST_SECOND); break;
      case 'L': opt.low_pct = atoi (optarg); break;
      case 'i': interactive = TRUE; break;
      case 'p': opt.plane = optarg; break;
      case 'c': opt.connector = optarg; break;
      case 'a': opt.alsa = optarg; break;
      case 'C': opt.channels = atoi (optarg); break;
      case 'v': opt.volume = g_ascii_strtod (optarg, NULL); break;
      default:
        fprintf (stderr, "usage: h96player [-i] [-u UA] [-r referer] [-H 'Name: value']... [-b secs]"
            " [-L low%%] [-p plane] [-c connector] [-a alsa-device] [-C channels] [-v 0..1] URI\n");
        return 2;
    }
  }
  if (optind != argc - 1) {
    say ("error: need exactly one URI");
    return 2;
  }
  const char *uri = argv[optind];
  if (!gst_uri_is_valid (uri)) {
    say ("error: not a URI: %s", uri);
    return 2;
  }

  /* playbin3, not playbin: after a seek hlsdemux2 re-creates its pads, and playbin's
   * input-selector reset then deadlocked against any query on playbin (NOTES §37) */
  pipe_ = make (g_getenv ("H96PLAYER_PLAYBIN") ? g_getenv ("H96PLAYER_PLAYBIN") : "playbin3");
  /* test hooks (NOTES §37): H96PLAYER_VSINK=fakesink, H96PLAYER_ASINK=fakesink */
  const char *vsn = g_getenv ("H96PLAYER_VSINK"), *asn = g_getenv ("H96PLAYER_ASINK");
  gboolean kms = !vsn || !strcmp (vsn, "kmssink");
  GstElement *vs = make (kms ? "kmssink" : vsn), *as = make (asn ? asn : "alsasink");
  vsink = vs;
  /* skip-vsync: the vendor kernel's legacy SetPlane already waits for vblank (NOTES §34) */
  if (kms) {
    gst_util_set_object_arg (G_OBJECT (vs), "plane-id", opt.plane);
    gst_util_set_object_arg (G_OBJECT (vs), "connector-id", opt.connector);
    g_object_set (vs, "skip-vsync", TRUE, NULL);
  } else {
    g_object_set (vs, "sync", TRUE, NULL);
  }
  if (!asn)
    g_object_set (as, "device", opt.alsa, NULL);
  else
    g_object_set (as, "sync", TRUE, NULL);
  if (!asn && opt.channels > 0) {
    /* downmix/upmix to a channel count the output takes (see -C above) */
    gchar *desc = g_strdup_printf ("audioconvert ! audioresample ! "
        "capsfilter caps=audio/x-raw,channels=%d", opt.channels);
    GError *e = NULL;
    GstElement *conv = gst_parse_bin_from_description (desc, FALSE, &e);
    g_free (desc);
    if (!conv) {
      say ("error: audio converter: %s", e ? e->message : "unknown");
      return 1;
    }
    GstElement *bin = gst_bin_new ("audio-out");
    gst_bin_add_many (GST_BIN (bin), conv, as, NULL);
    /* conv's unlinked ends: its first element's sink, its capsfilter's src */
    GstPad *in = gst_bin_find_unlinked_pad (GST_BIN (conv), GST_PAD_SINK);
    GstPad *out = gst_bin_find_unlinked_pad (GST_BIN (conv), GST_PAD_SRC);
    GstPad *csink = gst_ghost_pad_new ("sink", in), *csrc = gst_ghost_pad_new ("src", out);
    gst_element_add_pad (conv, csink);
    gst_element_add_pad (conv, csrc);
    gst_object_unref (in);
    gst_object_unref (out);
    gst_element_link (conv, as);
    GstPad *bsink = gst_element_get_static_pad (conv, "sink");
    gst_element_add_pad (bin, gst_ghost_pad_new ("sink", bsink));
    gst_object_unref (bsink);
    as = bin;
  }
  g_object_set (pipe_, "uri", uri, "video-sink", vs, "audio-sink", as,
      "volume", CLAMP (opt.volume, 0.0, 1.0), "buffer-duration", (gint64) opt.buffer_ns, NULL);
  /* no subtitles / visualisations: video + audio only */
  gint flags;
  g_object_get (pipe_, "flags", &flags, NULL);
  g_object_set (pipe_, "flags", (flags & ~(0x4 | 0x8)), NULL);   /* ~GST_PLAY_FLAG_TEXT|VIS */
  g_signal_connect (pipe_, "source-setup", G_CALLBACK (on_source_setup), NULL);
  g_signal_connect (pipe_, "element-setup", G_CALLBACK (on_element_setup), NULL);

  loop = g_main_loop_new (NULL, FALSE);
  GstBus *bus = gst_element_get_bus (pipe_);
  gst_bus_add_watch (bus, on_bus, NULL);
  gst_object_unref (bus);
  if (trace)
    g_timeout_add_seconds (5, on_stats, vs);
  if (interactive) {
    inbuf = g_string_new (NULL);
    g_unix_fd_add (0, G_IO_IN | G_IO_HUP | G_IO_ERR, on_stdin, NULL);
    g_timeout_add (1000, on_status, NULL);
  }
  g_unix_signal_add (SIGTERM, on_signal, NULL);
  g_unix_signal_add (SIGINT, on_signal, NULL);

  /* start PAUSED: prerolls and fills the prebuffer; buffering 100% switches to PLAYING. A
   * source that posts no buffering messages (plain file, progressive HTTP) goes straight on. */
  GstStateChangeReturn r = gst_element_set_state (pipe_, GST_STATE_PAUSED);
  if (r == GST_STATE_CHANGE_SUCCESS || r == GST_STATE_CHANGE_NO_PREROLL) {
    buffering = FALSE;
    gst_element_set_state (pipe_, GST_STATE_PLAYING);
  }
  if (r == GST_STATE_CHANGE_FAILURE) {
    say ("error: cannot start the pipeline");
    exit_code = 1;
  } else {
    g_main_loop_run (loop);
  }
  gst_element_set_state (pipe_, GST_STATE_NULL);
  gst_object_unref (pipe_);
  return exit_code;
}
