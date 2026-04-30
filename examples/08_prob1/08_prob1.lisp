; Largest palindrome made from products of two 3-digit numbers.
; Expected output: 906609

(defun rev3 ((n :int)) :int
  (let ((d0 :int (% n 10))
        (d1 :int (% (/ n 10) 10))
        (d2 :int (/ n 100)))
    (+ (* d0 100) (* d1 10) d2)))

(defun make-pal6 ((left :int)) :int
  (+ (* left 1000) (rev3 left)))

(begin
  (setq ans :int 0)
  (setq left :int 999)

  (loop while (and (= ans 0) (>= left 100)) do
    (begin
      (setq p :int (make-pal6 left))
      (setq a :int 990)

      (loop while (and (= ans 0) (>= a 110) (>= (* a 999) p)) do
        (begin
          (if (= (% p a) 0)
              (begin
                (setq b :int (/ p a))
                (if (and (>= b 100) (<= b 999))
                    (setq ans :int p)
                    0))
              0)
          (setq a :int (- a 11)))
        finally 0)

      (setq left :int (- left 1)))
    finally ans)

  (print ans)
  (halt))
