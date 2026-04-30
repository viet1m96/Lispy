; recursive Fibonacci, 32-bit int

(defun fib ((n :i64)) :i64
  (if (<= n 1)
      n
      (+ (fib (- n 1))
         (fib (- n 2)))))

(begin
  (print (fib 10))
  (halt))
