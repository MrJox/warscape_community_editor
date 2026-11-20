/*
 *  Copyright (c) 2026 The WebM project authors. All Rights Reserved.
 *
 *  Use of this source code is governed by a BSD-style license
 *  that can be found in the LICENSE file in the root of the source
 *  tree. An additional intellectual property rights grant can be found
 *  in the file PATENTS.  All contributing project authors may
 *  be found in the AUTHORS file in the root of the source tree.
 */

// This file is generated. Do not edit.
#ifndef VPX_SCALE_RTCD_H_
#define VPX_SCALE_RTCD_H_

#ifdef RTCD_C
#define RTCD_EXTERN
#else
#define RTCD_EXTERN extern
#endif

struct yv12_buffer_config;

#ifdef __cplusplus
extern "C" {
#endif

void vp8_yv12_copy_frame_c(const struct yv12_buffer_config *src_ybc, struct yv12_buffer_config *dst_ybc);
#define vp8_yv12_copy_frame vp8_yv12_copy_frame_c

void vp8_yv12_extend_frame_borders_c(struct yv12_buffer_config *ybf);
#define vp8_yv12_extend_frame_borders vp8_yv12_extend_frame_borders_c

void vpx_yv12_copy_y_c(const struct yv12_buffer_config *src_ybc, struct yv12_buffer_config *dst_ybc);
#define vpx_yv12_copy_y vpx_yv12_copy_y_c

void vpx_scale_rtcd(void);

#include "vpx_config.h"

#ifdef RTCD_C
static void setup_rtcd_internal(void)
{
}
#endif

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // VPX_SCALE_RTCD_H_
