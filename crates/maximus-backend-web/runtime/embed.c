#include <ecl/ecl.h>
#include <emscripten/emscripten.h>
#include <stdio.h>
#include <string.h>
#include "compiled-maxima.h"

static int maxima_initialized = 0;

static cl_object maxima_make_unspecial(cl_object symbol) {
    if (ECL_SYMBOLP(symbol)) {
        ecl_symbol_type_set(symbol, ecl_symbol_type(symbol) & ~ecl_stp_special);
    }
    return symbol;
}

static cl_object maxima_symbol(const char *name) {
    cl_object package = ecl_find_package("MAXIMA");
    int interned = 0;
    return ecl_find_symbol(ecl_make_simple_base_string(name, -1), package, &interned);
}

static cl_object maxima_utf8_string(const char *source) {
    return ecl_decode_from_cstring(source, (cl_fixnum)strlen(source),
                                   ecl_make_keyword("UTF-8"));
}

static int evaluate_form(cl_object form) {
    cl_object failed = ecl_make_keyword("MAXIMUS-EVALUATION-FAILED");
    cl_object bindings = ecl_read_from_cstring(
        "((error (lambda (e) (format *error-output* \"~&LISP ERROR: ~A~%\" e))))");
    cl_object guarded = cl_list(3, ecl_read_from_cstring("handler-bind"),
                                bindings, form);
    return cl_safe_eval(guarded, ECL_NIL, failed) != failed;
}

static int call_maxima(const char *name, const char *id, const char *source,
                       int has_source) {
    cl_object safe_call = ecl_read_from_cstring("cl-user::maximus-web-safe-call");
    cl_object target = cl_symbol_function(maxima_symbol(name));
    cl_object result;
    if (has_source) {
        result = cl_funcall(4, cl_symbol_function(safe_call), target,
                            ecl_make_simple_base_string(id, -1),
                            maxima_utf8_string(source));
    } else {
        result = cl_funcall(3, cl_symbol_function(safe_call), target,
                            ecl_make_simple_base_string(id, -1));
    }
    return result != ECL_NIL;
}

EMSCRIPTEN_KEEPALIVE int evaluate_lisp(const char *source) {
    return evaluate_form(ecl_read_from_cstring(source));
}

EMSCRIPTEN_KEEPALIVE int evaluate_maxima(const char *id, const char *source) {
    if (!maxima_initialized) {
        fprintf(stderr, "Maximus WASM: Maxima is not initialized\n");
        return 0;
    }
    return call_maxima("MX-EVAL", id, source, 1);
}

EMSCRIPTEN_KEEPALIVE int list_variables(const char *id) {
    if (!maxima_initialized) {
        fprintf(stderr, "Maximus WASM: Maxima is not initialized\n");
        return 0;
    }
    return call_maxima("MX-VARS", id, NULL, 0);
}

EMSCRIPTEN_KEEPALIVE int initialize_maxima(void) {
    if (maxima_initialized) {
        return 1;
    }

    char *args[] = {"maximus-wasm", NULL};
    cl_boot(1, args);
    if (!evaluate_lisp("(load \"/maxima/src/maxima-package.lisp\")")) {
        return 0;
    }

    ecl_def_c_function(ecl_read_from_cstring("maxima::make-unspecial"),
                       (cl_objectfn_fixed)maxima_make_unspecial, 1);
    if (!evaluate_lisp("(load \"/boot.lisp\")")) {
        return 0;
    }

    return 1;
}

EMSCRIPTEN_KEEPALIVE int load_maxima_file(const char *path) {
    if (load_compiled_maxima(path)) {
        return 1;
    }
    cl_object function = ecl_read_from_cstring("cl-user::maximus-web-load-file");
    return cl_funcall(2, cl_symbol_function(function), maxima_utf8_string(path)) != ECL_NIL;
}

EMSCRIPTEN_KEEPALIVE int finish_maxima(void) {
    if (!evaluate_lisp("(cl-user::maximus-web-finish)")) {
        return 0;
    }
    maxima_initialized = 1;
    return 1;
}
