// Optional native interoperability check. Does not encode or change the video.
// clang -fobjc-arc -framework Foundation mac-media-preflight.m -o media-preflight
// ./media-preflight /path/to/original.MOV /path/to/media-capabilities.json
#import <Foundation/Foundation.h>
#import <objc/message.h>
#import <dlfcn.h>

int main(int argc, char **argv) {
    @autoreleasepool {
        if (argc != 3) {
            fprintf(stderr, "Usage: media-preflight VIDEO CAPABILITIES_JSON\n");
            return 2;
        }
        dlopen("/System/Library/PrivateFrameworks/PhotosFormats.framework/PhotosFormats", RTLD_NOW);
        dlopen("/System/Library/Frameworks/Photos.framework/Photos", RTLD_NOW);
        Class capsClass = NSClassFromString(@"PFMediaCapabilities");
        Class sourceClass = NSClassFromString(@"PHMediaFormatConversionSource");
        Class requestClass = NSClassFromString(@"PHMediaFormatConversionRequest");
        if (!capsClass || !sourceClass || !requestClass) return 2;
        NSString *path = [NSString stringWithUTF8String:argv[1]];
        NSDictionary *attributes = [[NSFileManager defaultManager] attributesOfItemAtPath:path error:nil];
        NSData *data = [NSData dataWithContentsOfFile:[NSString stringWithUTF8String:argv[2]]];
        if (![attributes[NSFileSize] unsignedLongLongValue] || !data) return 2;
        id caps = ((id(*)(id, SEL, id))objc_msgSend)([capsClass alloc], NSSelectorFromString(@"initWithOpaqueRepresentation:"), data);
        id source = ((id(*)(id, SEL, id))objc_msgSend)(sourceClass, NSSelectorFromString(@"sourceForFileURL:"), [NSURL fileURLWithPath:path]);
        NSError *error = nil;
        id request = ((id(*)(id, SEL, id, id, NSError **))objc_msgSend)(requestClass, NSSelectorFromString(@"requestForSource:destinationCapabilities:error:"), source, caps, &error);
        if (!request || error) return 2;
        ((void(*)(id, SEL, BOOL))objc_msgSend)(request, NSSelectorFromString(@"setUseTransferBehaviorUserPreference:"), YES);
        id manager = [NSClassFromString(@"PHMediaFormatConversionManager") new];
        ((void(*)(id, SEL, id))objc_msgSend)(request, NSSelectorFromString(@"preflightWithConversionManager:"), manager);
        error = ((id(*)(id, SEL))objc_msgSend)(request, NSSelectorFromString(@"error"));
        if (error) { fprintf(stderr, "%s\n", error.description.UTF8String); return 2; }
        BOOL convert = ((BOOL(*)(id, SEL))objc_msgSend)(request, NSSelectorFromString(@"requiresFormatConversion"));
        BOOL hdr = ((BOOL(*)(id, SEL))objc_msgSend)(request, NSSelectorFromString(@"shouldExportAsHDR"));
        printf("{\"bytes\":%llu,\"requiresConversion\":%s,\"exportsHDR\":%s}\n",
               [attributes[NSFileSize] unsignedLongLongValue], convert ? "true" : "false", hdr ? "true" : "false");
        return convert ? 1 : 0;
    }
}
