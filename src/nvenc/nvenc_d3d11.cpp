#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <stdio.h>
#include <stdlib.h>
#include "nvEncodeAPI.h"

struct NvencEncoder {
    HMODULE hNvencDll;
    NV_ENCODE_API_FUNCTION_LIST fn;
    void* encoder;
    ID3D11Device* device;
    uint32_t width;
    uint32_t height;
    uint32_t fps;
    NV_ENC_OUTPUT_PTR bitstreamBuffers[4];
    uint32_t currentBufferIdx;
    NV_ENC_REGISTERED_PTR registeredTex;
};

typedef NVENCSTATUS (NVENCAPI *PNvEncodeAPICreateInstance)(NV_ENCODE_API_FUNCTION_LIST *functionList);

extern "C" __declspec(dllexport) void* nvenc_create(ID3D11Device* device, uint32_t width, uint32_t height, uint32_t fps, uint32_t bitrate_kbps, int* out_err) {
    if (out_err) *out_err = 0;
    if (!device || width == 0 || height == 0 || fps == 0) {
        if (out_err) *out_err = -1;
        return NULL;
    }

    HMODULE hDll = LoadLibraryA("nvEncodeAPI64.dll");
    if (!hDll) {
        if (out_err) *out_err = -2;
        return NULL;
    }

    PNvEncodeAPICreateInstance createInst = (PNvEncodeAPICreateInstance)GetProcAddress(hDll, "NvEncodeAPICreateInstance");
    if (!createInst) {
        if (out_err) *out_err = -3;
        FreeLibrary(hDll);
        return NULL;
    }

    NvencEncoder* enc = (NvencEncoder*)calloc(1, sizeof(NvencEncoder));
    enc->hNvencDll = hDll;
    enc->device = device;
    enc->width = width;
    enc->height = height;
    enc->fps = fps;

    enc->fn.version = NV_ENCODE_API_FUNCTION_LIST_VER;
    NVENCSTATUS st = createInst(&enc->fn);
    if (st != NV_ENC_SUCCESS) {
        if (out_err) *out_err = 100 + st;
        free(enc);
        FreeLibrary(hDll);
        return NULL;
    }

    NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS openParams = { 0 };
    openParams.version = NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER;
    openParams.deviceType = NV_ENC_DEVICE_TYPE_DIRECTX;
    openParams.device = device;
    openParams.apiVersion = NVENCAPI_VERSION;

    st = enc->fn.nvEncOpenEncodeSessionEx(&openParams, &enc->encoder);
    if (st != NV_ENC_SUCCESS) {
        if (out_err) *out_err = 200 + st;
        free(enc);
        FreeLibrary(hDll);
        return NULL;
    }

    NV_ENC_PRESET_CONFIG presetConfig = { 0 };
    presetConfig.version = NV_ENC_PRESET_CONFIG_VER;
    presetConfig.presetCfg.version = NV_ENC_CONFIG_VER;

    st = enc->fn.nvEncGetEncodePresetConfigEx(enc->encoder, NV_ENC_CODEC_H264_GUID, NV_ENC_PRESET_P4_GUID, NV_ENC_TUNING_INFO_LOW_LATENCY, &presetConfig);
    if (st != NV_ENC_SUCCESS) {
        if (out_err) *out_err = 300 + st;
        enc->fn.nvEncDestroyEncoder(enc->encoder);
        free(enc);
        FreeLibrary(hDll);
        return NULL;
    }

    presetConfig.presetCfg.rcParams.rateControlMode = NV_ENC_PARAMS_RC_CBR;
    presetConfig.presetCfg.rcParams.averageBitRate = bitrate_kbps * 1000;
    presetConfig.presetCfg.rcParams.maxBitRate = bitrate_kbps * 1000;
    presetConfig.presetCfg.gopLength = fps * 2;
    presetConfig.presetCfg.frameIntervalP = 1;

    NV_ENC_INITIALIZE_PARAMS initParams = { 0 };
    initParams.version = NV_ENC_INITIALIZE_PARAMS_VER;
    initParams.encodeGUID = NV_ENC_CODEC_H264_GUID;
    initParams.presetGUID = NV_ENC_PRESET_P4_GUID;
    initParams.encodeWidth = width;
    initParams.encodeHeight = height;
    initParams.darWidth = width;
    initParams.darHeight = height;
    initParams.frameRateNum = fps;
    initParams.frameRateDen = 1;
    initParams.enablePTD = 1;
    initParams.encodeConfig = &presetConfig.presetCfg;
    initParams.tuningInfo = NV_ENC_TUNING_INFO_LOW_LATENCY;

    st = enc->fn.nvEncInitializeEncoder(enc->encoder, &initParams);
    if (st != NV_ENC_SUCCESS) {
        if (out_err) *out_err = 400 + st;
        enc->fn.nvEncDestroyEncoder(enc->encoder);
        free(enc);
        FreeLibrary(hDll);
        return NULL;
    }

    for (int i = 0; i < 4; i++) {
        NV_ENC_CREATE_BITSTREAM_BUFFER bb = { 0 };
        bb.version = NV_ENC_CREATE_BITSTREAM_BUFFER_VER;
        bb.size = 4 * 1024 * 1024;
        st = enc->fn.nvEncCreateBitstreamBuffer(enc->encoder, &bb);
        if (st != NV_ENC_SUCCESS) {
            if (out_err) *out_err = 500 + st;
            enc->fn.nvEncDestroyEncoder(enc->encoder);
            free(enc);
            FreeLibrary(hDll);
            return NULL;
        }
        enc->bitstreamBuffers[i] = bb.bitstreamBuffer;
    }

    return enc;
}

extern "C" __declspec(dllexport) void* nvenc_register_texture(void* handle, ID3D11Texture2D* texture) {
    if (!handle || !texture) return NULL;
    NvencEncoder* enc = (NvencEncoder*)handle;

    D3D11_TEXTURE2D_DESC desc;
    texture->GetDesc(&desc);

    NV_ENC_REGISTER_RESOURCE reg = { 0 };
    reg.version = NV_ENC_REGISTER_RESOURCE_VER;
    reg.resourceType = NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX;
    reg.width = desc.Width;
    reg.height = desc.Height;
    reg.pitch = desc.Width * 4;
    reg.resourceToRegister = texture;
    reg.bufferFormat = (desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM) ? NV_ENC_BUFFER_FORMAT_ARGB : NV_ENC_BUFFER_FORMAT_ABGR;

    if (enc->fn.nvEncRegisterResource(enc->encoder, &reg) != NV_ENC_SUCCESS) {
        return NULL;
    }
    enc->registeredTex = reg.registeredResource;
    return reg.registeredResource;
}

extern "C" __declspec(dllexport) int nvenc_encode_frame(void* handle, void* reg_resource, int force_idr, uint8_t** out_data, uint32_t* out_size) {
    if (!handle || !reg_resource || !out_data || !out_size) return -1;
    NvencEncoder* enc = (NvencEncoder*)handle;

    NV_ENC_MAP_INPUT_RESOURCE map = { 0 };
    map.version = NV_ENC_MAP_INPUT_RESOURCE_VER;
    map.registeredResource = reg_resource;

    if (enc->fn.nvEncMapInputResource(enc->encoder, &map) != NV_ENC_SUCCESS) {
        return -2;
    }

    NV_ENC_OUTPUT_PTR bsBuf = enc->bitstreamBuffers[enc->currentBufferIdx];
    enc->currentBufferIdx = (enc->currentBufferIdx + 1) % 4;

    NV_ENC_PIC_PARAMS pic = { 0 };
    pic.version = NV_ENC_PIC_PARAMS_VER;
    pic.inputWidth = enc->width;
    pic.inputHeight = enc->height;
    pic.inputPitch = enc->width * 4;
    pic.inputBuffer = map.mappedResource;
    pic.outputBitstream = bsBuf;
    pic.bufferFmt = map.mappedBufferFmt;
    pic.pictureStruct = NV_ENC_PIC_STRUCT_FRAME;
    if (force_idr) {
        pic.encodePicFlags = NV_ENC_PIC_FLAG_FORCEIDR | NV_ENC_PIC_FLAG_OUTPUT_SPSPPS;
    }

    NVENCSTATUS encStatus = enc->fn.nvEncEncodePicture(enc->encoder, &pic);
    if (encStatus != NV_ENC_SUCCESS && encStatus != NV_ENC_ERR_NEED_MORE_INPUT) {
        enc->fn.nvEncUnmapInputResource(enc->encoder, map.mappedResource);
        return -3;
    }

    NV_ENC_LOCK_BITSTREAM lock = { 0 };
    lock.version = NV_ENC_LOCK_BITSTREAM_VER;
    lock.outputBitstream = bsBuf;
    lock.doNotWait = 0;

    if (enc->fn.nvEncLockBitstream(enc->encoder, &lock) != NV_ENC_SUCCESS) {
        enc->fn.nvEncUnmapInputResource(enc->encoder, map.mappedResource);
        return -4;
    }

    *out_data = (uint8_t*)lock.bitstreamBufferPtr;
    *out_size = lock.bitstreamSizeInBytes;

    enc->fn.nvEncUnlockBitstream(enc->encoder, bsBuf);
    enc->fn.nvEncUnmapInputResource(enc->encoder, map.mappedResource);
    return 0;
}

extern "C" __declspec(dllexport) void nvenc_destroy(void* handle) {
    if (!handle) return;
    NvencEncoder* enc = (NvencEncoder*)handle;

    if (enc->registeredTex) {
        enc->fn.nvEncUnregisterResource(enc->encoder, enc->registeredTex);
    }
    for (int i = 0; i < 4; i++) {
        if (enc->bitstreamBuffers[i]) {
            enc->fn.nvEncDestroyBitstreamBuffer(enc->encoder, enc->bitstreamBuffers[i]);
        }
    }
    if (enc->encoder) {
        enc->fn.nvEncDestroyEncoder(enc->encoder);
    }
    if (enc->hNvencDll) {
        FreeLibrary(enc->hNvencDll);
    }
    free(enc);
}