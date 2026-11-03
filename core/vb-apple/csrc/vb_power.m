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
 * Keeping a long run at full speed.
 *
 * macOS puts an app whose windows are hidden -- behind another app, on
 * another desktop, or with the screen locked overnight -- into App Nap:
 * its threads drop to background priority, which on Apple silicon means the
 * efficiency cores only, and its timers are coalesced. An analysis left
 * running like that ran about five times slower than in front of the user,
 * with every gauge in Activity Monitor reading "idle". The system may also
 * sleep while nobody touches the machine.
 *
 * A process activity is how an app tells the system the work was asked for
 * and is still wanted: no App Nap and no idle sleep while it is held. The
 * display may still sleep; the work does not need it.
 */
#import <Foundation/Foundation.h>

#include <pthread.h>
#include <pthread/qos.h>

void *vb_power_begin(const char *reason) {
    @autoreleasepool {
        NSString *why = reason ? [NSString stringWithUTF8String:reason] : nil;
        id<NSObject> token = [[NSProcessInfo processInfo]
            beginActivityWithOptions:(NSActivityUserInitiated | NSActivityIdleSystemSleepDisabled)
                              reason:why ?: @"Video Butler is working"];
        return (__bridge_retained void *)token;
    }
}

void vb_power_end(void *token) {
    if (!token) {
        return;
    }
    @autoreleasepool {
        id<NSObject> activity = (__bridge_transfer id<NSObject>)token;
        [[NSProcessInfo processInfo] endActivity:activity];
    }
}

/* The calling thread, and the threads it goes on to create, at the priority
 * of work the user is waiting for rather than background work. */
void vb_power_user_initiated_thread(void) {
    pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0);
}

/* How hot the machine is, as the system judges it: 0 nominal, 1 fair,
 * 2 serious, 3 critical (NSProcessInfoThermalState). Cheap enough to ask
 * between units of work. */
int vb_power_thermal_state(void) {
    return (int)[[NSProcessInfo processInfo] thermalState];
}
