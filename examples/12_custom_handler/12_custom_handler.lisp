(defun __default_input_handler () :int
  (begin
    (print (read-input-data))
    (handler-done)))

(setq i :int 0)

(loop while (< i 1000) do
  (setq i :int (+ i 1))
  finally
  (halt))
