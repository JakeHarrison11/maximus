(setf (logical-pathname-translations "SYS") '(("**;*.*" "/ecl/**/*.*")))
(setf *default-pathname-defaults* #p"/maxima/src/")
(defvar *maxima-build-time* (multiple-value-list (get-decoded-time)))
(export '*maxima-build-time*)
(setf *read-default-float-format* 'double-float)

;; Source loading cannot run ECL's native C-INLINE compiler. embed.c supplies
;; the symbol-flag operation provided by ecl-port.lisp on native builds.
(defun maximus-web-load-file (path)
  (handler-case
      (let ((*package* (find-package :cl-user)))
        (cond
          ((string= (pathname-name path) "ecl-port")
           (unless (fboundp 'maxima::make-unspecial)
             (error "The ECL embedding bridge is missing MAKE-UNSPECIAL"))
           (si:trap-fpe 'floating-point-underflow nil)
           (si:trap-fpe 'floating-point-overflow t))
          ((string= (pathname-name path) "server")
           (defvar maxima::$in_netmath nil)
           (defvar maxima::$show_openplot t)
           (defun maxima::setup-client (&rest args)
             (declare (ignore args))
             (error "Native socket clients are unavailable in browser Maxima"))
           (defun maxima::close-client ()
             (error "Native sockets are unavailable in browser Maxima"))
           (defun maxima::open-socket (&rest args)
             (declare (ignore args))
             (error "Native sockets are unavailable in browser Maxima")))
          ((string= (pathname-name path) "defsystem")
           ;; CMP is the ECL native compiler and is absent from the WASM build.
           (let* ((*load-truename* (pathname path))
                  (*load-pathname* *load-truename*))
             (with-open-file (input *load-truename*)
               (loop for form = (read input nil :eof)
                     until (eq form :eof)
                     unless (and (consp form)
                                 (eq (first form) 'require)
                                 (consp (second form))
                                 (eq (first (second form)) 'quote)
                                 (symbolp (second (second form)))
                                 (string= (symbol-name (second (second form))) "CMP"))
                       do (eval form)))))
          (t (load path)))
        t)
    (error (condition)
      (format *error-output* "~&Maximus startup error in ~A: ~A~%" path condition)
      nil)))

(defun maximus-web-finish ()
  (setf maxima::*autoconf-prefix* "/maxima"
        maxima::*maxima-source-root* "/maxima"
        ;; Optional share groups are mounted after boot and may be added later.
        maxima::$file_search_cache nil
        *standard-input* (make-string-input-stream "")
        *query-io* (make-two-way-stream (make-string-input-stream "") *standard-output*)
        *terminal-io* *query-io*)
  (maxima::initialize-runtime-globals)
  (load "/driver.lisp"))

;; The native driver owns the event-frame and evaluation protocol. Keep it
;; shared with the native backend rather than maintaining a second implementation.
(in-package :cl-user)
(defun maximus-web-safe-call (function &rest arguments)
  (handler-case
      (progn (apply function arguments) t)
    (error (condition)
      (format *error-output* "~&Maximus WASM driver error: ~A~%" condition)
      nil)))
