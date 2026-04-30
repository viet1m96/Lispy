;recursive factorial, 32-bit int

(defun fact ((n :i64)) :i64
  (if (<= n 1)
      1
      (* n (fact (- n 1)))))

(begin
  (print (fact 20))
  (halt))
