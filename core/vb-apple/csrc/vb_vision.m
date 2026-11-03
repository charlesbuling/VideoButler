// Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.

/*
 * Apple's Vision framework, behind three plain C functions.
 *
 * Rust never sees an Objective-C object, for the same reason it never sees a
 * libav struct: the framework's types are the part that changes between OS
 * releases, and keeping them on this side means one Rust source builds
 * against every SDK.
 *
 * What this buys the product is perception with nothing to download: scene
 * labels for a still, and faces with an appearance signature for grouping the
 * same person across clips. Both run on the device, both work offline, and
 * both are already on every Mac and iPhone the app supports.
 *
 * Every function returns a malloc'd JSON string that the caller frees with
 * vb_vision_free, or NULL with a reason written to `err`.
 */
#import <CoreImage/CoreImage.h>
#import <Foundation/Foundation.h>
#import <Vision/Vision.h>

#include <stdlib.h>
#include <string.h>

static char *copy_string(NSString *text) {
    const char *utf8 = [text UTF8String];
    if (!utf8) {
        return NULL;
    }
    size_t length = strlen(utf8) + 1;
    char *out = malloc(length);
    if (out) {
        memcpy(out, utf8, length);
    }
    return out;
}

static void set_err(char *err, size_t err_len, NSString *message) {
    if (!err || err_len == 0) {
        return;
    }
    const char *utf8 = [message UTF8String];
    if (!utf8) {
        utf8 = "unknown error";
    }
    strncpy(err, utf8, err_len - 1);
    err[err_len - 1] = '\0';
}

static char *encode(id object, char *err, size_t err_len) {
    NSError *error = nil;
    NSData *data = [NSJSONSerialization dataWithJSONObject:object options:0 error:&error];
    if (!data) {
        set_err(err, err_len, error.localizedDescription ?: @"could not encode the result");
        return NULL;
    }
    NSString *text = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    return copy_string(text);
}

/* A still on disk, as a request handler. */
static VNImageRequestHandler *handler_for(const char *path, char *err, size_t err_len) {
    NSString *file = [NSString stringWithUTF8String:path];
    if (!file) {
        set_err(err, err_len, @"the path is not valid UTF-8");
        return nil;
    }
    NSURL *url = [NSURL fileURLWithPath:file];
    if (![[NSFileManager defaultManager] fileExistsAtPath:file]) {
        set_err(err, err_len, [NSString stringWithFormat:@"%@ is not there", file]);
        return nil;
    }
    return [[VNImageRequestHandler alloc] initWithURL:url options:@{}];
}

int vb_vision_available(void) {
    /* The classes exist from macOS 10.15 and iOS 13, which is below both
     * deployment targets, so this is a sanity check rather than a gate. */
    return NSClassFromString(@"VNClassifyImageRequest") != nil &&
                   NSClassFromString(@"VNDetectFaceRectanglesRequest") != nil
               ? 1
               : 0;
}

/*
 * What is in this frame, as labels.
 *
 * Vision's own classifier over more than a thousand everyday categories:
 * beach, dog, concert, food. It is not a vision-language model and does not
 * write sentences -- the caller turns labels into a description and records
 * that this is what produced it.
 */
char *vb_vision_classify(const char *path, double min_confidence, int max_labels, char *err,
                         size_t err_len) {
    @autoreleasepool {
        VNImageRequestHandler *handler = handler_for(path, err, err_len);
        if (!handler) {
            return NULL;
        }

        VNClassifyImageRequest *request = [[VNClassifyImageRequest alloc] init];
        NSError *error = nil;
        if (![handler performRequests:@[ request ] error:&error]) {
            set_err(err, err_len, error.localizedDescription ?: @"classification failed");
            return NULL;
        }

        NSMutableArray *labels = [NSMutableArray array];
        for (VNClassificationObservation *observation in request.results) {
            if (observation.confidence < min_confidence) {
                continue;
            }
            if (max_labels > 0 && (int)labels.count >= max_labels) {
                break;
            }
            [labels addObject:@{
                @"label" : observation.identifier ?: @"",
                @"confidence" : @(observation.confidence)
            }];
        }
        return encode(@{@"labels" : labels}, err, err_len);
    }
}

/*
 * Faces in this frame, each with an appearance signature.
 *
 * The signature is Vision's image feature print, computed over the face's own
 * rectangle. Feature prints measure visual similarity; they are not face
 * identity vectors, and two photographs of one person in different light can
 * sit further apart than two people in the same light. The caller must treat
 * a grouping made from these as a suggestion and say so -- calling it
 * recognition would be a claim this cannot support.
 *
 * Rectangles are in Vision's normalised coordinates, origin bottom-left.
 */
char *vb_vision_faces(const char *path, char *err, size_t err_len) {
    @autoreleasepool {
        VNImageRequestHandler *handler = handler_for(path, err, err_len);
        if (!handler) {
            return NULL;
        }

        VNDetectFaceRectanglesRequest *faces = [[VNDetectFaceRectanglesRequest alloc] init];
        NSError *error = nil;
        if (![handler performRequests:@[ faces ] error:&error]) {
            set_err(err, err_len, error.localizedDescription ?: @"face detection failed");
            return NULL;
        }

        NSMutableArray *found = [NSMutableArray array];
        for (VNFaceObservation *face in faces.results) {
            CGRect box = face.boundingBox;
            NSMutableArray *signature = [NSMutableArray array];

            /* The feature print is computed over the face alone: over the
             * whole frame it would describe the room, and two people in one
             * kitchen would look like the same subject. */
            VNGenerateImageFeaturePrintRequest *print =
                [[VNGenerateImageFeaturePrintRequest alloc] init];
            print.regionOfInterest = box;
            NSError *printError = nil;
            if ([handler performRequests:@[ print ] error:&printError]) {
                VNFeaturePrintObservation *observation = print.results.firstObject;
                if (observation) {
                    NSData *data = observation.data;
                    if (observation.elementType == VNElementTypeFloat) {
                        const float *values = (const float *)data.bytes;
                        NSUInteger count = data.length / sizeof(float);
                        for (NSUInteger i = 0; i < count; i++) {
                            [signature addObject:@(values[i])];
                        }
                    } else if (observation.elementType == VNElementTypeDouble) {
                        const double *values = (const double *)data.bytes;
                        NSUInteger count = data.length / sizeof(double);
                        for (NSUInteger i = 0; i < count; i++) {
                            [signature addObject:@(values[i])];
                        }
                    }
                }
            }

            [found addObject:@{
                @"x" : @(box.origin.x),
                @"y" : @(box.origin.y),
                @"w" : @(box.size.width),
                @"h" : @(box.size.height),
                @"confidence" : @(face.confidence),
                @"signature" : signature
            }];
        }
        return encode(@{@"faces" : found}, err, err_len);
    }
}

/*
 * Crop a region out of a still and write it as a JPEG.
 *
 * For the album: a thumbnail of the subject rather than of the whole frame.
 * The rectangle is in Vision's normalised coordinates, as returned above.
 */
int vb_vision_crop(const char *path, const char *dest, double x, double y, double w, double h,
                   double padding, char *err, size_t err_len) {
    @autoreleasepool {
        NSString *file = [NSString stringWithUTF8String:path];
        NSString *out = [NSString stringWithUTF8String:dest];
        if (!file || !out) {
            set_err(err, err_len, @"the path is not valid UTF-8");
            return -1;
        }
        CIImage *image = [CIImage imageWithContentsOfURL:[NSURL fileURLWithPath:file]];
        if (!image) {
            set_err(err, err_len, [NSString stringWithFormat:@"%@ could not be read", file]);
            return -1;
        }

        CGRect extent = image.extent;
        /* Vision's origin is bottom-left, and so is Core Image's, so the
         * rectangle converts by scaling alone. */
        CGFloat px = x * extent.size.width;
        CGFloat py = y * extent.size.height;
        CGFloat pw = w * extent.size.width;
        CGFloat ph = h * extent.size.height;
        CGFloat padx = pw * padding;
        CGFloat pady = ph * padding;
        CGRect crop = CGRectIntersection(
            CGRectMake(px - padx, py - pady, pw + 2 * padx, ph + 2 * pady), extent);
        if (CGRectIsNull(crop) || crop.size.width < 8 || crop.size.height < 8) {
            set_err(err, err_len, @"the region is outside the image or too small to crop");
            return -1;
        }

        CIImage *cropped = [image imageByCroppingToRect:crop];
        CIContext *context = [CIContext context];
        NSError *error = nil;
        BOOL ok = [context writeJPEGRepresentationOfImage:cropped
                                                    toURL:[NSURL fileURLWithPath:out]
                                               colorSpace:cropped.colorSpace
                                                  options:@{}
                                                    error:&error];
        if (!ok) {
            set_err(err, err_len, error.localizedDescription ?: @"the crop could not be written");
            return -1;
        }
        return 0;
    }
}

void vb_vision_free(char *text) {
    free(text);
}
