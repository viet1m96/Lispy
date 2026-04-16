; Calculate Fibonacci by recursion

(defun fib (n)
  (if (<= n 1)
      n
      (+ (fib (- n 1))
         (fib (- n 2)))))

(begin
  (print (fib 8))
  (halt))
