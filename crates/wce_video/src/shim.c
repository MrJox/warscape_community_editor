/* A small C layer over libvpx's VP8 codec interfaces, so that Rust needs no mirror of libvpx's structs (vpx_image_t,
 * vpx_codec_enc_cfg_t, vpx_codec_cx_pkt_t): it passes plain planes, numbers and byte slices. */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "vpx/vp8cx.h"
#include "vpx/vp8dx.h"
#include "vpx/vpx_decoder.h"
#include "vpx/vpx_encoder.h"

/* ---- decoder ---- */

typedef struct wce_vp8_dec {
  vpx_codec_ctx_t ctx;
  vpx_codec_iter_t iter;
} wce_vp8_dec;

typedef struct wce_planes {
  const uint8_t *y, *u, *v;
  int y_stride, u_stride, v_stride;
  int width, height;
} wce_planes;

wce_vp8_dec *wce_vp8_dec_new(int threads) {
  wce_vp8_dec *d = (wce_vp8_dec *)calloc(1, sizeof(*d));
  vpx_codec_dec_cfg_t cfg;
  if (!d) return NULL;
  memset(&cfg, 0, sizeof(cfg));
  cfg.threads = threads > 0 ? (unsigned int)threads : 1;
  if (vpx_codec_dec_init(&d->ctx, vpx_codec_vp8_dx(), &cfg, 0) != VPX_CODEC_OK) {
    free(d);
    return NULL;
  }
  return d;
}

/* 0 on success, else a vpx_codec_err_t. */
int wce_vp8_dec_decode(wce_vp8_dec *d, const uint8_t *data, size_t len) {
  d->iter = NULL;
  return (int)vpx_codec_decode(&d->ctx, data, (unsigned int)len, NULL, 0);
}

/* 1 and the planes when the last decode gave a frame (they stay valid until the next decode), else 0. */
int wce_vp8_dec_frame(wce_vp8_dec *d, wce_planes *out) {
  vpx_image_t *img = vpx_codec_get_frame(&d->ctx, &d->iter);
  if (!img || img->fmt != VPX_IMG_FMT_I420) return 0;
  out->y = img->planes[VPX_PLANE_Y];
  out->u = img->planes[VPX_PLANE_U];
  out->v = img->planes[VPX_PLANE_V];
  out->y_stride = img->stride[VPX_PLANE_Y];
  out->u_stride = img->stride[VPX_PLANE_U];
  out->v_stride = img->stride[VPX_PLANE_V];
  out->width = (int)img->d_w;
  out->height = (int)img->d_h;
  return 1;
}

const char *wce_vp8_dec_error(wce_vp8_dec *d) { return vpx_codec_error_detail(&d->ctx) ? vpx_codec_error_detail(&d->ctx) : vpx_codec_error(&d->ctx); }

void wce_vp8_dec_free(wce_vp8_dec *d) {
  if (!d) return;
  vpx_codec_destroy(&d->ctx);
  free(d);
}

/* ---- encoder ---- */

typedef struct wce_vp8_enc_settings {
  int width, height;
  int fps_num, fps_den;
  int target_kbps;     /* the rate's cap in constrained quality */
  int cq_level;        /* 0..63, lower is better; < 0: plain VBR at target_kbps */
  int min_q, max_q;    /* 0..63 */
  int cpu_used;        /* -16..16: 0 slowest and best for the "good" deadline */
  int threads;
  int keyframe_max;    /* frames between forced key frames at most */
  int lag_in_frames;   /* look-ahead, for alt-ref frames (0..25) */
  int deadline_best;   /* 1: VPX_DL_BEST_QUALITY, 0: VPX_DL_GOOD_QUALITY */
} wce_vp8_enc_settings;

typedef struct wce_vp8_enc {
  vpx_codec_ctx_t ctx;
  vpx_image_t img;
  vpx_codec_iter_t iter;
  unsigned long deadline;
} wce_vp8_enc;

wce_vp8_enc *wce_vp8_enc_new(const wce_vp8_enc_settings *s) {
  vpx_codec_enc_cfg_t cfg;
  wce_vp8_enc *e;
  if (vpx_codec_enc_config_default(vpx_codec_vp8_cx(), &cfg, 0) != VPX_CODEC_OK) return NULL;
  cfg.g_w = (unsigned int)s->width;
  cfg.g_h = (unsigned int)s->height;
  cfg.g_timebase.num = s->fps_den;
  cfg.g_timebase.den = s->fps_num;
  cfg.g_threads = s->threads > 0 ? (unsigned int)s->threads : 1;
  cfg.g_lag_in_frames = (unsigned int)(s->lag_in_frames < 0 ? 0 : s->lag_in_frames);
  cfg.g_error_resilient = 0;
  cfg.g_pass = VPX_RC_ONE_PASS;
  cfg.rc_end_usage = s->cq_level >= 0 ? VPX_CQ : VPX_VBR;
  cfg.rc_target_bitrate = (unsigned int)s->target_kbps;
  cfg.rc_min_quantizer = (unsigned int)s->min_q;
  cfg.rc_max_quantizer = (unsigned int)s->max_q;
  cfg.kf_mode = VPX_KF_AUTO;
  cfg.kf_min_dist = 0;
  cfg.kf_max_dist = (unsigned int)s->keyframe_max;
  e = (wce_vp8_enc *)calloc(1, sizeof(*e));
  if (!e) return NULL;
  if (vpx_codec_enc_init(&e->ctx, vpx_codec_vp8_cx(), &cfg, 0) != VPX_CODEC_OK) {
    free(e);
    return NULL;
  }
  vpx_codec_control(&e->ctx, VP8E_SET_CPUUSED, s->cpu_used);
  if (s->cq_level >= 0) vpx_codec_control(&e->ctx, VP8E_SET_CQ_LEVEL, s->cq_level);
  if (s->lag_in_frames > 0) vpx_codec_control(&e->ctx, VP8E_SET_ENABLEAUTOALTREF, 1);
  e->deadline = s->deadline_best ? VPX_DL_BEST_QUALITY : VPX_DL_GOOD_QUALITY;
  return e;
}

/* Encodes one I420 frame (y == NULL flushes). 0 on success, else a vpx_codec_err_t. The packets it made follow from
 * wce_vp8_enc_packet. */
int wce_vp8_enc_encode(wce_vp8_enc *e, const uint8_t *y, const uint8_t *u, const uint8_t *v, int y_stride, int uv_stride,
                       int64_t pts, int force_key) {
  vpx_codec_err_t r;
  e->iter = NULL;
  if (!y) {
    r = vpx_codec_encode(&e->ctx, NULL, pts, 1, 0, e->deadline);
  } else {
    memset(&e->img, 0, sizeof(e->img));
    vpx_img_wrap(&e->img, VPX_IMG_FMT_I420, e->ctx.config.enc->g_w, e->ctx.config.enc->g_h, 1, (unsigned char *)y);
    e->img.planes[VPX_PLANE_Y] = (unsigned char *)y;
    e->img.planes[VPX_PLANE_U] = (unsigned char *)u;
    e->img.planes[VPX_PLANE_V] = (unsigned char *)v;
    e->img.stride[VPX_PLANE_Y] = y_stride;
    e->img.stride[VPX_PLANE_U] = uv_stride;
    e->img.stride[VPX_PLANE_V] = uv_stride;
    r = vpx_codec_encode(&e->ctx, &e->img, pts, 1, force_key ? VPX_EFLAG_FORCE_KF : 0, e->deadline);
  }
  return (int)r;
}

/* 1 and the next compressed frame (valid until the next encode), else 0. */
int wce_vp8_enc_packet(wce_vp8_enc *e, const uint8_t **data, size_t *len, int64_t *pts, int *is_key) {
  const vpx_codec_cx_pkt_t *p;
  while ((p = vpx_codec_get_cx_data(&e->ctx, &e->iter)) != NULL) {
    if (p->kind != VPX_CODEC_CX_FRAME_PKT) continue;
    *data = (const uint8_t *)p->data.frame.buf;
    *len = p->data.frame.sz;
    *pts = p->data.frame.pts;
    *is_key = (p->data.frame.flags & VPX_FRAME_IS_KEY) != 0;
    return 1;
  }
  return 0;
}

const char *wce_vp8_enc_error(wce_vp8_enc *e) { return vpx_codec_error_detail(&e->ctx) ? vpx_codec_error_detail(&e->ctx) : vpx_codec_error(&e->ctx); }

void wce_vp8_enc_free(wce_vp8_enc *e) {
  if (!e) return;
  vpx_codec_destroy(&e->ctx);
  free(e);
}
